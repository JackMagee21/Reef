mod cd;
mod echo;
mod exit;
mod false_cmd;
mod pwd;
mod true_cmd;

use crate::io::Io;
use crate::shell::Shell;

pub type Builtin = fn(&mut Shell, &[String], &mut Io) -> i32;

const BUILTINS: &[(&str, Builtin)] = &[
    ("cd", cd::cd),
    ("echo", echo::echo),
    ("exit", exit::exit),
    ("false", false_cmd::false_cmd),
    ("pwd", pwd::pwd),
    ("true", true_cmd::true_cmd),
];

pub fn lookup(name: &str) -> Option<Builtin> {
    BUILTINS.iter().find(|(n, _)| *n == name).map(|&(_, f)| f)
}
