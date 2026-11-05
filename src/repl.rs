//! Running Reef with no `-c`: an interactive prompt, or commands read from piped stdin.

use std::borrow::Cow;
use std::io::{ErrorKind, IsTerminal, Read};
use std::path::{MAIN_SEPARATOR, Path};

use reedline::{
    Color, DefaultHinter, FileBackedHistory, HISTORY_SIZE, Prompt, PromptEditMode,
    PromptHistorySearch, Reedline, Signal, ValidationResult, Validator,
};

use crate::exec;
use crate::parser::{self, ParseError};
use crate::platform;
use crate::shell::Shell;
use crate::status;
use crate::streams::Io;

/// Run one complete command (its status lands in `$?`). Returns true when
/// `exit` or a script's syntax error asked Reef to stop. The prompt and piped
/// mode both go through here, so they can't drift apart.
fn run_one(src: &str, sh: &mut Shell, io: &mut Io) -> bool {
    exec::run_source(src, sh, io);
    sh.stop_requested
}

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
pub fn start(sh: &mut Shell, io: &mut Io) -> i32 {
    if std::io::stdin().is_terminal() {
        interactive(sh, io)
    } else {
        piped(sh, io)
    }
}

// shortcut: no tab completion or highlighting yet; those come with the editor work
fn interactive(sh: &mut Shell, io: &mut Io) -> i32 {
    // The faded "ghost" suggestion from history as you type; → accepts it
    let mut editor = Reedline::create()
        .with_validator(Box::new(Continue))
        .with_hinter(Box::new(
            DefaultHinter::default().with_style(Color::DarkGray.italic()),
        ));
    match saved_history(sh) {
        Ok(history) => editor = editor.with_history(Box::new(history)),
        // Up-arrow and suggestions still work, just for this session
        Err(e) => eprintln!("reef: history won't be saved: {e}"),
    }

    let title = Color::Cyan
        .bold()
        .paint(format!("reef {}", env!("CARGO_PKG_VERSION")));
    // Same faded grey as the history suggestions
    let hint = Color::DarkGray.paint("run `reef --update` to get the newest version");
    if let Err(e) = io.write_out(format!("{title}\n{hint}\n").as_bytes()) {
        eprintln!("reef: {e}");
    }

    sh.interactive = true;
    loop {
        // Rebuilt every time: `cd` changes the directory it shows
        match editor.read_line(&UnixPrompt::new(sh)) {
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

/// History in `<data dir>/history`, so Up-arrow and suggestions remember
/// commands from earlier sessions.
fn saved_history(sh: &Shell) -> Result<FileBackedHistory, String> {
    let dir = platform::data_dir(sh).ok_or("no home directory")?;
    FileBackedHistory::with_file(HISTORY_SIZE, dir.join("history")).map_err(|e| e.to_string())
}

/// `jmt@link:~/src$ `, colored like Debian and Ubuntu's bash prompt.
/// `#` instead of `$` for root.
struct UnixPrompt {
    /// `user@host:dir`, with color codes
    left: String,
    indicator: &'static str,
}

impl UnixPrompt {
    fn new(sh: &Shell) -> Self {
        let user = sh.first_var(platform::USER_VARS).unwrap_or("user");
        let home = sh.first_var(platform::HOME_VARS);
        let user_host = format!("{user}@{}", platform::host_name(sh));
        UnixPrompt {
            left: format!(
                "{}:{}",
                Color::Green.bold().paint(user_host),
                Color::Blue.bold().paint(tilde(sh.cwd(), home))
            ),
            indicator: if user == "root" { "# " } else { "$ " },
        }
    }
}

/// `cwd` with the home directory written as `~`, like bash's `\w`.
fn tilde(cwd: &Path, home: Option<&str>) -> String {
    match home
        .filter(|h| !h.is_empty())
        .and_then(|h| cwd.strip_prefix(h).ok())
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~{MAIN_SEPARATOR}{}", rest.display()),
        None => cwd.display().to_string(),
    }
}

impl Prompt for UnixPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.left)
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed(self.indicator)
    }

    /// The `> ` bash shows when a command continues on the next line
    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("> ")
    }

    fn render_prompt_history_search_indicator(&self, _search: PromptHistorySearch) -> Cow<'_, str> {
        Cow::Borrowed("(search) ")
    }

    /// The `$ ` and `> ` in the terminal's normal color, like bash
    fn get_indicator_color(&self) -> Color {
        Color::Default
    }

    fn get_prompt_multiline_color(&self) -> Color {
        Color::Default
    }
}

/// What the prompt does with one event from the line editor; `Some(status)` means stop.
/// Kept out of the loop so it can be tested without a real terminal.
fn on_signal(signal: Signal, sh: &mut Shell, io: &mut Io) -> Option<i32> {
    match signal {
        Signal::Success(line) => {
            let stop = run_one(&line, sh, io);
            // `echo -n hi` must show before the next prompt is drawn
            if let Err(e) = io.flush() {
                eprintln!("reef: {e}");
            }
            stop.then_some(sh.last_status)
        }
        // Ctrl-C at the prompt cancels the line, like bash
        Signal::CtrlC => {
            sh.last_status = status::INTERRUPTED;
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
        if run_one(&std::mem::take(&mut cmd), sh, io) {
            break;
        }
    }
    sh.last_status
}

/// One line without its `\n` / `\r\n`, or `None` at end of input.
/// Reads a byte at a time, like bash does on a pipe: slow, but never takes
/// bytes that belong to the next line. Invalid UTF-8 becomes `\u{FFFD}`
/// rather than stopping the whole script.
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
    Ok(Some(String::from_utf8_lossy(&line).into_owned()))
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
    fn the_prompt_shows_home_as_a_tilde() {
        let home = Some("/home/jmt");
        assert_eq!(tilde(Path::new("/home/jmt"), home), "~");
        assert_eq!(
            tilde(&Path::new("/home/jmt").join("src").join("reef"), home),
            format!("~{MAIN_SEPARATOR}src{MAIN_SEPARATOR}reef")
        );
        // only whole folder names: /home/jmtx isn't inside /home/jmt
        assert_eq!(tilde(Path::new("/home/jmtx"), home), "/home/jmtx");
        assert_eq!(tilde(Path::new("/tmp"), home), "/tmp");
        assert_eq!(tilde(Path::new("/tmp"), None), "/tmp");
        assert_eq!(tilde(Path::new("/tmp"), Some("")), "/tmp");
    }

    #[test]
    fn the_prompt_is_user_at_host_colon_dir() {
        let mut sh = Shell::new();
        for v in platform::USER_VARS {
            sh.set_var(v, "jmt".into(), false);
        }
        let p = UnixPrompt::new(&sh);
        let plain = String::from_utf8(strip_ansi(p.left.as_bytes())).unwrap();
        assert!(plain.starts_with("jmt@"), "{plain}");
        assert!(plain.contains(':'), "{plain}");
        assert_eq!(p.indicator, "$ ");
        for v in platform::USER_VARS {
            sh.set_var(v, "root".into(), false);
        }
        assert_eq!(UnixPrompt::new(&sh).indicator, "# ");
    }

    /// The prompt without its color codes (`ESC [ ... m`).
    fn strip_ansi(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut in_code = false;
        for &b in bytes {
            match (in_code, b) {
                (false, 0x1b) => in_code = true,
                (true, b'm') => in_code = false,
                (true, _) => {}
                (false, b) => out.push(b),
            }
        }
        out
    }

    #[test]
    fn a_syntax_error_does_not_stop_the_prompt() {
        let mut sh = Shell::new();
        sh.interactive = true;
        assert_eq!(step(Signal::Success("echo 'oops".into()), &mut sh), None);
        assert_eq!(sh.last_status, 2);
    }
}
