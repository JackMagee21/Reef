mod builtins;
mod io;
mod lexer;
mod platform;
mod repl;
mod shell;
mod word;

use io::Io;
use lexer::{Op, Token, TokenKind};
use shell::Shell;
use word::Word;

fn main() {
    // Ctrl-C goes to every process on the console. Reef ignores it so only the
    // running program stops, never the shell itself.
    // Once loops and `read` exist, this must set a flag they check, or Ctrl-C can't stop them.
    if let Err(e) = ctrlc::set_handler(|| {}) {
        eprintln!("reef: could not install Ctrl-C handler: {e}");
    }

    // args_os, not args: args() panics on a non-UTF-8 argument
    let args: Vec<String> = std::env::args_os()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let mut sh = Shell::new();
    let mut io = Io::terminal();

    sh.last_status = match (args.get(1).map(String::as_str), args.get(2)) {
        (None, _) => repl::run(&mut sh, &mut io),
        (Some("-c"), Some(src)) => run(src, &mut sh, &mut io),
        (Some("-c"), None) => {
            eprintln!("reef: -c: option requires an argument");
            2
        }
        (Some(_), _) => {
            eprintln!("usage: reef [-c <command>]");
            2
        }
    };

    // process::exit skips destructors, so flush explicitly or `echo -n` output can be lost
    if let Err(e) = io.flush() {
        eprintln!("reef: {e}");
    }
    std::process::exit(sh.last_status);
}

// Placeholder until the parser exists: one line is one simple command.
// The spec tests stay the same when it's replaced.
fn run(src: &str, sh: &mut Shell, io: &mut Io) -> i32 {
    let words = match lexer::lex(src)
        .map_err(|e| e.to_string())
        .and_then(|t| only_words(src, t))
    {
        Ok(words) => words,
        Err(e) => {
            let _ = io.write_err(format!("reef: syntax error: {e}\n").as_bytes());
            // POSIX: a script stops at a syntax error instead of running the lines after it
            if !sh.interactive {
                sh.exit_requested = true;
            }
            return 2;
        }
    };
    let argv: Vec<String> = words.iter().map(Word::to_plain).collect();
    let Some(name) = argv.first() else {
        return 0;
    };

    // Builtins first, then PATH
    if let Some(builtin) = builtins::lookup(name) {
        return builtin(sh, &argv, io);
    }

    let Some(path) = platform::find_command(name, sh) else {
        // If stderr itself is broken there's nowhere left to report it
        let _ = io.write_err(format!("reef: {name}: command not found\n").as_bytes());
        return 127;
    };

    // Anything a builtin wrote must reach the console before the child writes after it
    if let Err(e) = io.flush() {
        let _ = io.write_err(format!("reef: {e}\n").as_bytes());
        return 1;
    }

    let (stdin, stdout, stderr) = match io.for_child() {
        Ok(handles) => handles,
        Err(e) => {
            let _ = io.write_err(format!("reef: {name}: {e}\n").as_bytes());
            return 126;
        }
    };

    let status = std::process::Command::new(&path)
        .args(&argv[1..])
        .current_dir(&sh.cwd)
        .env_clear()
        .envs(sh.exported_vars())
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr)
        .status();

    match status {
        Ok(s) => platform::exit_status(s),
        // Found but couldn't start: no permission, a directory, a bad `#!` line
        Err(e) => {
            let _ = io.write_err(format!("reef: {name}: {e}\n").as_bytes());
            if e.kind() == std::io::ErrorKind::NotFound {
                127
            } else {
                126
            }
        }
    }
}

// Placeholder until the parser exists: one simple command, so only words.
// Newlines at either end are fine (piped mode ends every command with one);
// any other operator is reported with its column instead of being run wrongly.
fn only_words(src: &str, tokens: Vec<Token>) -> Result<Vec<Word>, String> {
    let is_newline = |t: &Token| t.kind == TokenKind::Op(Op::Newline);
    let start = tokens.iter().position(|t| !is_newline(t)).unwrap_or(0);
    let end = tokens
        .iter()
        .rposition(|t| !is_newline(t))
        .map_or(0, |i| i + 1);
    let tokens: Vec<Token> = tokens.into_iter().take(end).skip(start).collect();
    // An io number (`2` in `2>x`) is always followed by its operator, so this reports `>`
    if let Some(t) = tokens.iter().find(|t| matches!(t.kind, TokenKind::Op(_))) {
        let column = src[..t.span.start].chars().count() + 1;
        let text = src[t.span.start..t.span.end].escape_debug();
        return Err(format!("`{text}` (column {column}) is not supported yet"));
    }
    Ok(tokens
        .into_iter()
        .filter_map(|t| match t.kind {
            TokenKind::Word(w) => Some(w),
            _ => None,
        })
        .collect())
}
