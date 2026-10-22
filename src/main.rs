mod ast;
mod builtins;
mod exec;
mod lexer;
mod parser;
mod platform;
mod repl;
mod shell;
mod status;
mod streams;
mod word;

use shell::Shell;
use streams::Io;

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

    let exit_code = match (args.get(1).map(String::as_str), args.get(2)) {
        (None, _) => repl::start(&mut sh, &mut io),
        (Some("-c"), Some(src)) => exec::run_source(src, &mut sh, &mut io),
        (Some("-c"), None) => {
            eprintln!("reef: -c: option requires an argument");
            status::SYNTAX_ERROR
        }
        (Some(_), _) => {
            eprintln!("usage: reef [-c <command>]");
            status::SYNTAX_ERROR
        }
    };

    // process::exit skips destructors, so flush explicitly or `echo -n` output can be lost
    if let Err(e) = io.flush() {
        eprintln!("reef: {e}");
    }
    std::process::exit(exit_code);
}
