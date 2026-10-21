mod platform;
mod shell;

use shell::Shell;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut sh = Shell::new();
    sh.last_status = match args.get(1).map(String::as_str) {
        Some("-c") => run(args.get(2).map(String::as_str).unwrap_or(""), &mut sh),
        _ => {
            eprintln!("usage: reef -c <command>");
            2
        }
    };
    std::process::exit(sh.last_status);
}

// Placeholder until the lexer and parser exist. The spec tests stay the same when it's replaced.
fn run(src: &str, sh: &mut Shell) -> i32 {
    let argv: Vec<&str> = src.split_whitespace().collect();
    let Some((name, rest)) = argv.split_first() else {
        return 0;
    };
    let Some(path) = platform::find_command(name, sh.var("PATH").unwrap_or_default(), &sh.cwd)
    else {
        eprintln!("reef: {name}: command not found");
        return 127;
    };
    let status = std::process::Command::new(&path)
        .args(rest)
        .current_dir(&sh.cwd)
        .env_clear()
        .envs(sh.exported_vars())
        .status();
    match status {
        Ok(s) => platform::exit_status(s),
        Err(e) => {
            eprintln!("reef: {name}: {e}");
            126
        }
    }
}
