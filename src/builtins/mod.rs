mod cd;
mod echo;
mod exit;
mod false_cmd;
mod pwd;
mod true_cmd;
mod type_cmd;
mod unset;

use crate::shell::Shell;
use crate::streams::Io;

pub type Builtin = fn(&mut Shell, &[String], &mut Io) -> i32;

const BUILTINS: &[(&str, Builtin)] = &[
    ("cd", cd::cd),
    ("echo", echo::echo),
    ("exit", exit::exit),
    ("false", false_cmd::false_cmd),
    ("pwd", pwd::pwd),
    ("true", true_cmd::true_cmd),
    ("type", type_cmd::type_cmd),
    ("unset", unset::unset),
];

pub fn lookup(name: &str) -> Option<Builtin> {
    BUILTINS.iter().find(|(n, _)| *n == name).map(|&(_, f)| f)
}

/// Write a builtin's output. Returns the builtin's status: 0, or 1 after
/// reporting `reef: <name>: write error: ...` if the output couldn't be written.
fn print(io: &mut Io, name: &str, text: &str) -> i32 {
    match io.write_out(text.as_bytes()) {
        Ok(()) => 0,
        Err(e) => {
            io.error(format_args!("{name}: write error: {e}"));
            1
        }
    }
}
