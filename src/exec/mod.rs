//! Runs the command structure the parser built.

mod redirect;

use crate::ast::{AndOr, Command, Connector, List, Pipeline, Redirect};
use crate::builtins;
use crate::io::Io;
use crate::lexer::Op;
use crate::platform;
use crate::shell::Shell;
use crate::word::Word;

/// Parsed but not runnable yet, checked before anything runs, so
/// `echo a; echo b | c` fails as a whole instead of running half of it.
pub fn unsupported(list: &List) -> Option<String> {
    list.iter().find_map(|item| {
        if item.background {
            return Some("background jobs (`&`) are not supported yet".into());
        }
        item.and_or.pipelines().find_map(|p| {
            if p.commands.len() > 1 {
                return Some("pipelines (`|`) are not supported yet".into());
            }
            p.commands.iter().find_map(unsupported_command)
        })
    })
}

fn unsupported_command(c: &Command) -> Option<String> {
    c.redirects()
        .iter()
        .find_map(unsupported_redirect)
        .or_else(|| c.body().and_then(unsupported))
}

fn unsupported_redirect(r: &Redirect) -> Option<String> {
    if matches!(r.op, Op::DLess | Op::DLessDash) {
        // The body is the lines after the command; reading those comes later
        return Some(format!("heredocs (`{r}`) are not supported yet"));
    }
    if r.fd.is_some_and(|fd| fd > 2) {
        return Some(format!(
            "file descriptors above 2 (`{r}`) are not supported yet"
        ));
    }
    if matches!(r.op, Op::GreatAnd | Op::LessAnd) && r.target.as_unquoted() == Some("-") {
        return Some(format!(
            "closing file descriptors (`{r}`) is not supported yet"
        ));
    }
    None
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
    // With redirects, the command (and everything inside a `{ }` or `( )`)
    // runs with its own copy of the streams; without, it shares the caller's
    let mut redirected;
    let io = if c.redirects().is_empty() {
        io
    } else {
        // Output still buffered for the screen must come out before anything
        // written through the new streams, e.g. `echo -n a; echo b >&2`
        let _ = io.flush();
        match redirect::apply(c.redirects(), sh, io) {
            Ok(new) => {
                redirected = new;
                &mut redirected
            }
            // The redirect failed, so the command doesn't run
            Err(message) => {
                io.error(message);
                return 1;
            }
        }
    };
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
        io.error(format_args!("{name}: command not found"));
        return 127;
    };

    // Anything a builtin wrote must reach the console before the child writes after it
    if let Err(e) = io.flush() {
        io.error(e);
        return 1;
    }

    let (stdin, stdout, stderr) = match io.for_child() {
        Ok(handles) => handles,
        Err(e) => {
            io.error(format_args!("{name}: {e}"));
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
            io.error(format_args!("{name}: {e}"));
            if e.kind() == std::io::ErrorKind::NotFound {
                127
            } else {
                126
            }
        }
    }
}
