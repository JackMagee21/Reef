//! Running Reef with no `-c`: an interactive prompt, or commands read from piped stdin.

use std::io::{ErrorKind, IsTerminal, Read};

use reedline::{
    DefaultPrompt, DefaultPromptSegment, Reedline, Signal, ValidationResult, Validator,
};

use crate::io::Io;
use crate::parser::{self, ParseError};
use crate::platform;
use crate::shell::Shell;

/// True when `src` stops in the middle of a command (`echo a &&`, `{ echo`, an
/// open quote), so the next line belongs to the same command.
// ponytail: parses twice (here and in run); fine while inputs are lines a person typed
fn unfinished(src: &str) -> bool {
    matches!(parser::parse(src), Err(ParseError::Incomplete(_)))
}

/// Tells the line editor when Enter should start a new line instead of running.
struct Continue;

impl Validator for Continue {
    fn validate(&self, line: &str) -> ValidationResult {
        // The editor's text has no final newline; add it so a line ending in `\` continues
        if unfinished(&format!("{line}\n")) {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Complete
        }
    }
}

/// Interactive when stdin is a console, otherwise run each line of stdin like a script.
/// Returns the status Reef should exit with.
pub fn run(sh: &mut Shell, io: &mut Io) -> i32 {
    if std::io::stdin().is_terminal() {
        interactive(sh, io)
    } else {
        piped(sh, io)
    }
}

// shortcut: no history file or completion yet; those come with the editor work
fn interactive(sh: &mut Shell, io: &mut Io) -> i32 {
    let mut editor = Reedline::create().with_validator(Box::new(Continue));
    let prompt = DefaultPrompt::new(
        DefaultPromptSegment::Basic("reef".into()),
        DefaultPromptSegment::Empty,
    );

    sh.interactive = true;
    loop {
        match editor.read_line(&prompt) {
            Ok(signal) => {
                if let Some(status) = on_signal(signal, sh, io) {
                    return status;
                }
            }
            Err(e) => {
                eprintln!("reef: line editor: {e}");
                return 1;
            }
        }
    }
}

/// What the prompt does with one event from the line editor; `Some(status)` means stop.
/// Kept out of the loop so it can be tested without a real terminal.
fn on_signal(signal: Signal, sh: &mut Shell, io: &mut Io) -> Option<i32> {
    match signal {
        Signal::Success(line) => {
            sh.last_status = crate::run(&line, sh, io);
            // `echo -n hi` must show before the next prompt is drawn
            if let Err(e) = io.flush() {
                eprintln!("reef: {e}");
            }
            sh.exit_requested.then_some(sh.last_status)
        }
        // Ctrl-C at the prompt cancels the line, like bash
        Signal::CtrlC => {
            sh.last_status = 130;
            None
        }
        Signal::CtrlD => Some(sh.last_status),
        // Signal is non-exhaustive: newer reedline versions may add events Reef doesn't use yet
        _ => None,
    }
}

fn piped(sh: &mut Shell, io: &mut Io) -> i32 {
    // Not std::io::stdin(): it reads ahead in big chunks and its lock would be held
    // for the whole loop, so a program run by one line couldn't read the lines after it.
    let mut stdin = match platform::stdin_file() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("reef: reading stdin: {e}");
            return 1;
        }
    };
    // One command, which may span several lines: `echo 'a` + `b'`, or a line ending in `\`
    let mut cmd = String::new();
    loop {
        match read_line(&mut stdin) {
            Ok(Some(line)) => {
                cmd.push_str(&line);
                cmd.push('\n');
                if unfinished(&cmd) {
                    continue;
                }
            }
            Ok(None) if cmd.is_empty() => break,
            // End of input in the middle of a command: run reports the syntax error and stops
            Ok(None) => {}
            Err(e) => {
                eprintln!("reef: reading stdin: {e}");
                return 1;
            }
        }
        sh.last_status = crate::run(&std::mem::take(&mut cmd), sh, io);
        if sh.exit_requested {
            break;
        }
    }
    sh.last_status
}

/// One line without its `\n` / `\r\n`, or `None` at end of input.
/// Reads a byte at a time, like bash does on a pipe: slow, but never takes
/// bytes that belong to the next line.
fn read_line(r: &mut impl Read) -> std::io::Result<Option<String>> {
    let mut line = Vec::new();
    let mut byte = [0u8];
    loop {
        match r.read(&mut byte) {
            Ok(0) if line.is_empty() => return Ok(None),
            Ok(0) => break, // last line had no `\n`
            Ok(_) if byte[0] == b'\n' => break,
            Ok(_) => line.push(byte[0]),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(signal: Signal, sh: &mut Shell) -> Option<i32> {
        on_signal(signal, sh, &mut Io::terminal())
    }

    #[test]
    fn a_command_keeps_the_prompt_going() {
        let mut sh = Shell::new();
        assert_eq!(step(Signal::Success("false".into()), &mut sh), None);
        assert_eq!(sh.last_status, 1);
    }

    #[test]
    fn exit_stops_the_prompt() {
        let mut sh = Shell::new();
        assert_eq!(step(Signal::Success("exit 4".into()), &mut sh), Some(4));
    }

    #[test]
    fn ctrl_c_cancels_the_line_and_sets_130() {
        let mut sh = Shell::new();
        assert_eq!(step(Signal::CtrlC, &mut sh), None);
        assert_eq!(sh.last_status, 130);
    }

    #[test]
    fn ctrl_d_exits_with_the_last_status() {
        let mut sh = Shell::new();
        step(Signal::Success("false".into()), &mut sh);
        assert_eq!(step(Signal::CtrlD, &mut sh), Some(1));
    }

    #[test]
    fn enter_continues_an_unfinished_command() {
        let incomplete = |line| matches!(Continue.validate(line), ValidationResult::Incomplete);
        assert!(incomplete("echo a &&"));
        assert!(incomplete("{ echo a"));
        assert!(incomplete("echo 'a"));
        assert!(incomplete("echo a \\"));
        assert!(incomplete("echo a &&\n"));
        // complete, or wrong (an error is reported, not waited on)
        assert!(!incomplete("echo a"));
        assert!(!incomplete("echo a && echo b"));
        assert!(!incomplete("echo )"));
    }

    #[test]
    fn a_syntax_error_does_not_stop_the_prompt() {
        let mut sh = Shell::new();
        sh.interactive = true;
        assert_eq!(step(Signal::Success("echo 'oops".into()), &mut sh), None);
        assert_eq!(sh.last_status, 2);
    }
}
