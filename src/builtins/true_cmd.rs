//! `true`: do nothing, successfully. (`true` is a Rust keyword, hence the file name.)

use crate::io::Io;
use crate::shell::Shell;

/// Ignores its arguments and returns 0.
pub fn true_cmd(_sh: &mut Shell, _argv: &[String], _io: &mut Io) -> i32 {
    0
}
