//! Runs the command structure the parser built.

use crate::ast::{AndOr, Command, Connector, List, Pipeline};
use crate::builtins;
use crate::io::Io;
use crate::platform;
use crate::shell::Shell;
use crate::word::Word;

/// Parsed but not runnable yet, checked before anything runs, so
/// `echo a; echo b | c` fails as a whole instead of running half of it.
/// Pipes, redirects and `&` each need new kinds of `Io` stream first.
pub fn unsupported(list: &List) -> Option<String> {
    list.iter().find_map(|item| {
        if item.background {
            return Some("background jobs (`&`) are not supported yet".into());
        }
        let pipelines =
            std::iter::once(&item.and_or.first).chain(item.and_or.rest.iter().map(|(_, p)| p));
        pipelines.into_iter().find_map(|p| {
            if p.commands.len() > 1 {
                return Some("pipelines (`|`) are not supported yet".into());
            }
            p.commands.iter().find_map(unsupported_command)
        })
    })
}

fn unsupported_command(c: &Command) -> Option<String> {
    let (redirects, body) = match c {
        Command::Simple { redirects, .. } => (redirects, None),
        Command::Subshell { body, redirects } | Command::Group { body, redirects } => {
            (redirects, Some(body))
        }
    };
    if let Some(r) = redirects.first() {
        let fd = r.fd.map(|n| n.to_string()).unwrap_or_default();
        let text = format!("{fd}{}{}", r.op.as_str(), r.target.to_plain());
        return Some(format!("redirects (`{text}`) are not supported yet"));
    }
    body.and_then(unsupported)
}

/// Run each item in order and return the last status. An empty list (a blank
/// line or only a comment) leaves `$?` as it was.
pub fn list(list: &List, sh: &mut Shell, io: &mut Io) -> i32 {
    for item in list {
        sh.last_status = and_or(&item.and_or, sh, io);
        if sh.exit_requested {
            break;
        }
    }
    sh.last_status
}

/// `a && b || c`: each pipeline after the first runs only if the status so far
/// says so (0 for `&&`, non-zero for `||`).
fn and_or(a: &AndOr, sh: &mut Shell, io: &mut Io) -> i32 {
    let mut status = pipeline(&a.first, sh, io);
    for (connector, p) in &a.rest {
        if sh.exit_requested {
            break;
        }
        // `exit` with no argument must see the status so far, like `$?` will
        sh.last_status = status;
        let run = match connector {
            Connector::And => status == 0,
            Connector::Or => status != 0,
        };
        if run {
            status = pipeline(p, sh, io);
        }
    }
    status
}

fn pipeline(p: &Pipeline, sh: &mut Shell, io: &mut Io) -> i32 {
    // ponytail: runs only the first command; `unsupported` rejects pipes until they exist
    let status = command(&p.commands[0], sh, io);
    if p.negated {
        (status == 0) as i32
    } else {
        status
    }
}

fn command(c: &Command, sh: &mut Shell, io: &mut Io) -> i32 {
    match c {
        Command::Simple { words, .. } => {
            let argv: Vec<String> = words.iter().map(Word::to_plain).collect();
            simple(&argv, sh, io)
        }
        Command::Group { body, .. } => list(body, sh, io),
        // A copy of the shell: `cd`, variables and `exit` inside only affect the copy
        Command::Subshell { body, .. } => list(body, &mut sh.clone(), io),
    }
}

/// One command: a builtin, or a program found on `PATH`.
fn simple(argv: &[String], sh: &mut Shell, io: &mut Io) -> i32 {
    let Some(name) = argv.first() else {
        return 0;
    };

    // Builtins first, then PATH
    if let Some(builtin) = builtins::lookup(name) {
        return builtin(sh, argv, io);
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
