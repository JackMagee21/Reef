//! Running Reef with no `-c`: an interactive prompt, or commands read from piped stdin.

use std::io::{ErrorKind, IsTerminal, Read};

use reedline::{DefaultPrompt, DefaultPromptSegment, Reedline, Signal};

use crate::io::Io;
use crate::platform;
use crate::shell::Shell;

/// Interactive when stdin is a console, otherwise run each line of stdin like a script.
/// Returns the status Reef should exit with.
pub fn run(sh: &mut Shell, io: &mut Io) -> i32 {
    if std::io::stdin().is_terminal() {
        interactive(sh, io)
    } else {
        piped(sh, io)
    }
}

// shortcut: no history file, completion or multi-line input yet; those come with the parser and editor work
fn interactive(sh: &mut Shell, io: &mut Io) -> i32 {
    let mut editor = Reedline::create();
    let prompt = DefaultPrompt::new(
        DefaultPromptSegment::Basic("reef".into()),
        DefaultPromptSegment::Empty,
    );

    loop {
        match editor.read_line(&prompt) {
            Ok(Signal::Success(line)) => {
                sh.last_status = crate::run(&line, sh, io);
                // `echo -n hi` must show before the next prompt is drawn
                if let Err(e) = io.flush() {
                    eprintln!("reef: {e}");
                }
                if sh.exit_requested {
                    return sh.last_status;
                }
            }
            // Ctrl-C at the prompt cancels the line, like bash
            Ok(Signal::CtrlC) => sh.last_status = 130,
            Ok(Signal::CtrlD) => return sh.last_status,
            // Signal is non-exhaustive: newer reedline versions may add events Reef doesn't use yet
            Ok(_) => {}
            Err(e) => {
                eprintln!("reef: line editor: {e}");
                return 1;
            }
        }
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
    loop {
        match read_line(&mut stdin) {
            Ok(Some(line)) => {
                sh.last_status = crate::run(&line, sh, io);
                if sh.exit_requested {
                    break;
                }
            }
            Ok(None) => break,
            Err(e) => {
                eprintln!("reef: reading stdin: {e}");
                return 1;
            }
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
