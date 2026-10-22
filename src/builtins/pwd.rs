//! `pwd`: print the shell's directory.

use crate::io::Io;
use crate::shell::Shell;

/// Prints `sh.cwd`, never the process's own directory. `-L`/`-P` are ignored:
/// `cd` already stores the real (physical) path.
pub fn pwd(sh: &mut Shell, _argv: &[String], io: &mut Io) -> i32 {
    match io.write_out(format!("{}\n", sh.cwd.display()).as_bytes()) {
        Ok(()) => 0,
        Err(e) => {
            let _ = io.write_err(format!("reef: pwd: write error: {e}\n").as_bytes());
            1
        }
    }
}
