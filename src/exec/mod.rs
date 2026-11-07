//! Runs source text: parses it, checks it, then walks the tree the parser built.

mod redirect;

use std::fmt;

use crate::ast::{AndOr, Command, Connector, List, Pipeline, Redirect};
use crate::builtins;
use crate::expand;
use crate::lexer::{Op, Span};
use crate::parser;
use crate::platform;
use crate::shell::{Shell, is_valid_name};
use crate::status;
use crate::streams::{Io, Stream, describe_error};
use crate::word::{Word, WordPart};

/// Parse and run one complete input: a line from the prompt, a `-c` string,
/// or a multi-line command from a script. Returns the status, which is also
/// left in `$?`.
pub fn run_source(src: &str, sh: &mut Shell, io: &mut Io) -> i32 {
    let program = match parser::parse(src) {
        Ok(program) => program,
        Err(e) => return refuse(sh, io, format_args!("{}: {}", e.label(), e.describe(src))),
    };
    if let Err((message, span)) = check_supported(&program) {
        let (line, pointer) = (parser::line_note(src, span), parser::pointer(src, span));
        return refuse(sh, io, format_args!("{message}{line}\n{pointer}"));
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

/// Why something can't run, and where in the input it is.
type Refusal = (String, Span);

/// Everything in `list` can run, or the message for the first thing that
/// can't (`not supported yet: background jobs (`&`)`). Checked before anything
/// runs, so `echo a; echo b &` fails as a whole instead of running half of it.
fn check_supported(list: &List) -> Result<(), Refusal> {
    for item in list {
        if let Some(amp) = item.background {
            return Err((not_yet("background jobs (`&`)"), amp));
        }
        for p in item.and_or.pipelines() {
            for c in &p.commands {
                if let Command::Simple { words, .. } = c {
                    words.iter().try_for_each(check_word)?;
                }
                c.redirects().iter().try_for_each(check_redirect)?;
                if let Some(body) = c.body() {
                    check_supported(body)?;
                }
            }
        }
    }
    Ok(())
}

/// `${x}` and `$?` can run; `${x:-default}` and friends can't yet. A
/// `$(...)` is parsed and checked like everything else.
fn check_word(w: &Word) -> Result<(), Refusal> {
    let refuse = |message| Err((message, w.span));
    for part in &w.parts {
        match part {
            WordPart::Param { name, .. } if name != "?" && !is_valid_name(name) => {
                return refuse(not_yet(format!("`${{{name}}}`")));
            }
            WordPart::CommandSub { src, .. } if src.starts_with('(') => {
                return refuse(not_yet(format!("arithmetic (`$({src})`)")));
            }
            // shortcut: errors inside point at the whole word; `src` doesn't know
            // where it starts in the line yet
            WordPart::CommandSub { src, .. } => {
                let inside = |message| format!("{message}, inside `$({src})`");
                let program = match parser::parse(src) {
                    Ok(program) => program,
                    Err(e) => return refuse(inside(format!("{}: {}", e.label(), e.message(src)))),
                };
                if let Err((message, _)) = check_supported(&program) {
                    return refuse(inside(message));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn check_redirect(r: &Redirect) -> Result<(), Refusal> {
    check_word(&r.target)?;
    let refuse = |what| Err((not_yet(what), r.span));
    if matches!(r.op, Op::DLess | Op::DLessDash) {
        // The body is the lines after the command; reading those comes later
        return refuse(format!("heredocs (`{r}`)"));
    }
    if r.fd.is_some_and(|fd| fd > 2) {
        return refuse(format!("file descriptors above 2 (`{r}`)"));
    }
    if matches!(r.op, Op::GreatAnd | Op::LessAnd) && r.target.as_unquoted() == Some("-") {
        return refuse(format!("closing file descriptors (`{r}`)"));
    }
    Ok(())
}

fn not_yet(what: impl fmt::Display) -> String {
    format!("not supported yet: {what}")
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
    let code = match p.commands.as_slice() {
        // On its own, a command runs in this shell, so `cd` and `x=1` stick
        [only] => command(only, sh, io),
        commands => piped(commands, sh, io),
    };
    if p.negated {
        status::negate(code)
    } else {
        code
    }
}

/// `a | b | c`: every command runs at the same time on its own thread, with
/// a copy of the shell (like `( )`), each one's stdout piped into the next
/// one's stdin. A thread runs a builtin itself, or starts a program and waits
/// for it. The status is the last command's.
///
/// Threads instead of `fork`, which Windows doesn't have: see design/0001.
fn piped(commands: &[Command], sh: &Shell, io: &mut Io) -> i32 {
    // Each command's streams start as copies of the pipeline's own
    let mut ios = match commands
        .iter()
        .map(|_| io.try_clone())
        .collect::<std::io::Result<Vec<Io>>>()
    {
        Ok(ios) => ios,
        Err(e) => {
            io.error(describe_error(&e));
            return 1;
        }
    };
    for i in 1..ios.len() {
        let (reader, writer) = match std::io::pipe() {
            Ok(ends) => ends,
            Err(e) => {
                io.error(format_args!("pipe: {}", describe_error(&e)));
                return 1;
            }
        };
        ios[i - 1].stdout = Stream::PipeOut(writer);
        ios[i].stdin = Stream::PipeIn(reader);
    }
    // Output already buffered for the screen comes before the pipeline's
    if let Err(e) = io.flush() {
        io.error(format_args!("write error: {e}"));
    }

    // Each thread owns its streams, so its pipe ends close the moment it
    // finishes. Reef keeps none, or `yes | head` would never end.
    std::thread::scope(|scope| {
        let threads: Vec<_> = commands
            .iter()
            .zip(ios)
            .map(|(c, mut own)| {
                let mut copy = sh.clone();
                scope.spawn(move || command(c, &mut copy, &mut own))
            })
            .collect();
        // A thread only fails by panicking, which has already printed why
        let codes: Vec<i32> = threads.into_iter().map(|t| t.join().unwrap_or(1)).collect();
        codes.last().copied().unwrap_or(0)
    })
}

/// A command with its redirects: they give it (and everything inside a `{ }`
/// or `( )`) its own copy of the streams. Without redirects it shares the caller's.
fn command(c: &Command, sh: &mut Shell, outer: &mut Io) -> i32 {
    if c.redirects().is_empty() {
        return run_command(c, sh, outer);
    }
    // Output still buffered for the screen must come out before anything
    // written through the new streams, e.g. `echo -n a; echo b >&2`
    // A broken screen shouldn't stop `> file` from working, so report and carry on
    if let Err(e) = outer.flush() {
        outer.error(format_args!("write error: {e}"));
    }
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
        Command::Simple { words, .. } => simple(words, sh, io),
        Command::Group { body, .. } => list(body, sh, io),
        // A copy of the shell: `cd`, variables and `exit` inside only affect the copy
        Command::Subshell { body, .. } => list(body, &mut sh.clone(), io),
    }
}

/// One command: `name=value` words, then a builtin or a program found on `PATH`.
///
/// With no command left after expansion, the assignments set shell variables
/// (left to right, so `x=1 y=$x` works). Before a program, they're only in its
/// environment.
fn simple(words: &[Word], sh: &mut Shell, io: &mut Io) -> i32 {
    // POSIX: with no command, the status is the last `$(...)`'s, else 0
    let substitutes = words.iter().any(Word::has_command_sub);
    let assignments: Vec<(String, Word)> = words.iter().map_while(Word::assignment).collect();
    let words = &words[assignments.len()..];

    // `export x=$y` must not split `$y`, like a plain `x=$y` (POSIX "declaration utility")
    let declares = words.first().and_then(Word::as_unquoted) == Some("export");
    let argv: Vec<String> = words
        .iter()
        .flat_map(|w| match w.assignment() {
            Some((name, value)) if declares => {
                vec![format!("{name}={}", expand::single(&value, sh, io))]
            }
            _ => expand::fields(w, sh, io),
        })
        .collect();

    let Some(name) = argv.first() else {
        for (name, value) in &assignments {
            let value = expand::single(value, sh, io);
            sh.set_var(name, value, false);
        }
        return if substitutes { sh.last_status } else { 0 };
    };
    let env: Vec<(String, String)> = assignments
        .into_iter()
        .map(|(name, value)| (name, expand::single(&value, sh, io)))
        .collect();

    // Builtins first, then PATH
    if let Some(builtin) = builtins::lookup(name) {
        if let Some((var, _)) = env.first() {
            // shortcut: would need setting and restoring the variable around the builtin
            io.error(format_args!(
                "not supported yet: assignments before a builtin (`{var}=... {name}`)"
            ));
            return status::SYNTAX_ERROR;
        }
        return builtin(sh, &argv, io);
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
        .envs(env)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr)
        .status();

    match result {
        Ok(s) => platform::exit_status(s),
        // The program is there, but the shell's directory was deleted under it
        Err(_) if !sh.cwd().is_dir() => {
            io.error(format_args!(
                "{name}: current directory {} no longer exists",
                sh.cwd().display()
            ));
            status::CANNOT_RUN
        }
        // Found but couldn't start: no permission, a directory, a bad `#!` line
        Err(e) => {
            io.error(format_args!("{name}: {}", describe_error(&e)));
            if e.kind() == std::io::ErrorKind::NotFound {
                status::NOT_FOUND
            } else {
                status::CANNOT_RUN
            }
        }
    }
}
