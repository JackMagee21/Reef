//! `pwd`: print the shell's directory.

use crate::io::Io;
use crate::shell::Shell;

/// Prints `sh.cwd`, never the process's own directory. `-L`/`-P` are ignored:
/// `cd` already stores the real (physical) path.
pub fn pwd(sh: &mut Shell, _argv: &[String], io: &mut Io) -> i32 {
    super::print(io, "pwd", &format!("{}\n", sh.cwd.display()))
}
