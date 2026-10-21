//! Commands Reef runs itself, instead of starting a separate program.

mod echo;

use crate::io::Io;
use crate::shell::Shell;

/// The shape every builtin has: the shell, the arguments (argv[0] is the
/// command name itself) and the streams in; the exit status out.
pub type Builtin = fn(&mut Shell, &[String], &mut Io) -> i32;

/// The only list of builtins in Reef. Completion, `type`, `which` and
/// "did you mean" will all read this same list later.
const BUILTINS: &[(&str, Builtin)] = &[("echo", echo::echo)];

/// Find a builtin by name.
pub fn lookup(name: &str) -> Option<Builtin> {
    BUILTINS.iter().find(|(n, _)| *n == name).map(|&(_, f)| f)
}
