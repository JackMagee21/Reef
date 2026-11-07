mod cd;
mod echo;
mod exit;
mod export;
mod false_cmd;
mod pwd;
mod true_cmd;
mod type_cmd;
mod unset;

use crate::shell::Shell;
use crate::status;
use crate::streams::Io;

pub type Builtin = fn(&mut Shell, &[String], &mut Io) -> i32;

const BUILTINS: &[(&str, Builtin)] = &[
    ("cd", cd::cd),
    ("echo", echo::echo),
    ("exit", exit::exit),
    ("export", export::export_cmd),
    ("false", false_cmd::false_cmd),
    ("pwd", pwd::pwd),
    ("true", true_cmd::true_cmd),
    ("type", type_cmd::type_cmd),
    ("unset", unset::unset),
    // Only work inside a shell. Fedora ships each as a program too
    // (`/usr/bin/alias` runs `builtin alias "$@"` in a throwaway `sh`), which
    // would do nothing and report success. Refusing beats silently doing nothing.
    ("alias", not_yet),
    ("bg", not_yet),
    ("fc", not_yet),
    ("fg", not_yet),
    ("getopts", not_yet),
    ("hash", not_yet),
    ("jobs", not_yet),
    ("read", not_yet),
    ("ulimit", not_yet),
    ("umask", not_yet),
    ("unalias", not_yet),
];

fn not_yet(_: &mut Shell, argv: &[String], io: &mut Io) -> i32 {
    io.error(format_args!("not supported yet: `{}`", argv[0]));
    status::SYNTAX_ERROR
}

pub fn lookup(name: &str) -> Option<Builtin> {
    BUILTINS.iter().find(|(n, _)| *n == name).map(|&(_, f)| f)
}

/// Write a builtin's output. Returns the builtin's status: 0, or 1 after
/// reporting `reef: <name>: write error: ...` if the output couldn't be written.
/// A pipe whose reader has gone (`echo a | true`) fails quietly, like bash.
fn print(io: &mut Io, name: &str, text: &str) -> i32 {
    match io.write_out(text.as_bytes()) {
        Ok(()) => 0,
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => 1,
        Err(e) => {
            io.error(format_args!("{name}: write error: {e}"));
            1
        }
    }
}
