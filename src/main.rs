mod builtins;
mod io;
mod platform;
mod repl;
mod shell;

use io::Io;
use shell::Shell;

fn main() {
    // Ctrl-C goes to every process on the console. Reef ignores it so only the
    // running program stops, never the shell itself.
    if let Err(e) = ctrlc::set_handler(|| {}) {
        eprintln!("reef: could not install Ctrl-C handler: {e}");
    }

    let args: Vec<String> = std::env::args().collect();
    let mut sh = Shell::new();
    let mut io = Io::terminal();

    sh.last_status = match args.get(1).map(String::as_str) {
        None => repl::run(&mut sh, &mut io),
        Some("-c") => run(
            args.get(2).map(String::as_str).unwrap_or(""),
            &mut sh,
            &mut io,
        ),
        Some(_) => {
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

// Placeholder until the lexer and parser exist. The spec tests stay the same when it's replaced.
fn run(src: &str, sh: &mut Shell, io: &mut Io) -> i32 {
    let argv: Vec<String> = src.split_whitespace().map(String::from).collect();
    let Some(name) = argv.first() else {
        return 0;
    };

    // Builtins first, then PATH
    if let Some(builtin) = builtins::lookup(name) {
        return builtin(sh, &argv, io);
    }

    let Some(path) = platform::find_command(name, sh.var("PATH").unwrap_or_default(), &sh.cwd)
    else {
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
        Err(e) => {
            let _ = io.write_err(format!("reef: {name}: {e}\n").as_bytes());
            126
        }
    }
}
