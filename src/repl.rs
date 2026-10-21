//! Running Reef with no `-c`: an interactive prompt, or commands read from piped stdin.

use std::io::{BufRead, IsTerminal};

use reedline::{DefaultPrompt, DefaultPromptSegment, Reedline, Signal};

use crate::io::Io;
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
    for line in std::io::stdin().lock().lines() {
        match line {
            Ok(line) => sh.last_status = crate::run(&line, sh, io),
            Err(e) => {
                eprintln!("reef: reading stdin: {e}");
                return 1;
            }
        }
    }
    sh.last_status
}
