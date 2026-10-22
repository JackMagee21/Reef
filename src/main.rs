mod ast;
mod builtins;
mod exec;
mod io;
mod lexer;
mod parser;
mod platform;
mod repl;
mod shell;
mod word;

use io::Io;
use shell::Shell;

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

/// Parse and run one complete input (a line, a `-c` string, or a multi-line command).
fn run(src: &str, sh: &mut Shell, io: &mut Io) -> i32 {
    let checked = parser::parse(src)
        .map_err(|e| e.describe(src))
        .and_then(|list| exec::unsupported(&list).map_or(Ok(list), Err));
    match checked {
        Ok(list) => exec::list(&list, sh, io),
        Err(message) => {
            io.error(format_args!("syntax error: {message}"));
            // POSIX: a script stops at a syntax error instead of running the lines after it
            if !sh.interactive {
                sh.exit_requested = true;
            }
            2
        }
    }
}
