//! Runs source text: parses it, checks it, then walks the tree the parser built.

mod redirect;

use std::fmt;

use crate::ast::{AndOr, Command, Connector, List, Pipeline, Redirect};
use crate::builtins;
use crate::lexer::Op;
use crate::parser;
use crate::platform;
use crate::shell::Shell;
use crate::status;
use crate::streams::Io;
use crate::word::Word;

/// Parse and run one complete input: a line from the prompt, a `-c` string,
/// or a multi-line command from a script. Returns the status, which is also
/// left in `$?`.
pub fn run_source(src: &str, sh: &mut Shell, io: &mut Io) -> i32 {
    let program = match parser::parse(src) {
        Ok(program) => program,
        Err(e) => return refuse(sh, io, format_args!("{}: {}", e.label(), e.describe(src))),
    };
    if let Err(what) = check_supported(&program) {
        return refuse(sh, io, format_args!("not supported yet: {what}"));
    }
    list(&program, sh, io)
}

/// Report an input that can't run at all, before any of it ran. Outside the
/// prompt this stops Reef too: POSIX says a script must not carry on past a
/// syntax error.
fn refuse(sh: &mut Shell, io: &mut Io, message: impl fmt::Display) -> i32 {
    io.error(message);
    if !sh.interactive {
        sh.stop_requested = true;
    }
    sh.last_status = status::SYNTAX_ERROR;
    status::SYNTAX_ERROR
}

/// Everything in `list` can run, or the first thing that can't yet (`pipelines (`|`)`).
/// Checked before anything runs, so `echo a; echo b | c` fails as a whole
/// instead of running half of it.
fn check_supported(list: &List) -> Result<(), String> {
    for item in list {
        if item.background {
            return Err("background jobs (`&`)".into());
        }
        for p in item.and_or.pipelines() {
            if p.commands.len() > 1 {
                return Err("pipelines (`|`)".into());
            }
            for c in &p.commands {
                c.redirects().iter().try_for_each(check_redirect)?;
                if let Some(body) = c.body() {
                    check_supported(body)?;
                }
            }
        }
    }
    Ok(())
}

fn check_redirect(r: &Redirect) -> Result<(), String> {
    if matches!(r.op, Op::DLess | Op::DLessDash) {
        // The body is the lines after the command; reading those comes later
        return Err(format!("heredocs (`{r}`)"));
    }
    if r.fd.is_some_and(|fd| fd > 2) {
        return Err(format!("file descriptors above 2 (`{r}`)"));
    }
    if matches!(r.op, Op::GreatAnd | Op::LessAnd) && r.target.as_unquoted() == Some("-") {
        return Err(format!("closing file descriptors (`{r}`)"));
    }
    Ok(())
}

/// Run each item in order and return the last status. An empty list (a blank
/// line or only a comment) leaves `$?` as it was.
fn list(items: &List, sh: &mut Shell, io: &mut Io) -> i32 {
    for item in items {
        sh.last_status = and_or(&item.and_or, sh, io);
        if sh.stop_requested {
            break;
        }
    }
    sh.last_status
}

/// `a && b || c`: each pipeline after the first runs only if the status so far
/// says so (0 for `&&`, non-zero for `||`).
fn and_or(a: &AndOr, sh: &mut Shell, io: &mut Io) -> i32 {
    let mut code = pipeline(&a.first, sh, io);
    for (connector, p) in &a.rest {
        if sh.stop_requested {
            break;
        }
        // `exit` with no argument must see the status so far, like `$?` will
        sh.last_status = code;
        let run = match connector {
            Connector::And => code == 0,
            Connector::Or => code != 0,
        };
        if run {
            code = pipeline(p, sh, io);
        }
    }
    code
}

fn pipeline(p: &Pipeline, sh: &mut Shell, io: &mut Io) -> i32 {
    // ponytail: runs only the first command; `check_supported` rejects pipes until they exist
    let code = command(&p.commands[0], sh, io);
    if p.negated {
        status::negate(code)
    } else {
        code
    }
}

/// A command with its redirects: they give it (and everything inside a `{ }`
/// or `( )`) its own copy of the streams. Without redirects it shares the caller's.
fn command(c: &Command, sh: &mut Shell, outer: &mut Io) -> i32 {
    if c.redirects().is_empty() {
        return run_command(c, sh, outer);
    }
    // Output still buffered for the screen must come out before anything
    // written through the new streams, e.g. `echo -n a; echo b >&2`
    let _ = outer.flush();
    match redirect::apply(c.redirects(), sh, outer) {
        Ok(mut own) => run_command(c, sh, &mut own),
        // The redirect failed, so the command doesn't run
        Err(message) => {
            outer.error(message);
            1
        }
    }
}

/// The command itself, with whatever streams it was given.
fn run_command(c: &Command, sh: &mut Shell, io: &mut Io) -> i32 {
    match c {
        Command::Simple { words, .. } => {
            let argv: Vec<String> = words.iter().map(Word::joined_text).collect();
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
        return status::NOT_FOUND;
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
            return status::CANNOT_RUN;
        }
    };

    let result = std::process::Command::new(&path)
        .args(&argv[1..])
        .current_dir(sh.cwd())
        .env_clear()
        .envs(sh.exported_vars())
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr)
        .status();

    match result {
        Ok(s) => platform::exit_status(s),
        // Found but couldn't start: no permission, a directory, a bad `#!` line
        Err(e) => {
            io.error(format_args!("{name}: {e}"));
            if e.kind() == std::io::ErrorKind::NotFound {
                status::NOT_FOUND
            } else {
                status::CANNOT_RUN
            }
        }
    }
}
