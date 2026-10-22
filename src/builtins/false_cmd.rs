//! `false`: do nothing, unsuccessfully. (`false` is a Rust keyword, hence the file name.)

use crate::shell::Shell;
use crate::streams::Io;

/// Ignores its arguments and returns 1.
pub fn false_cmd(_sh: &mut Shell, _argv: &[String], _io: &mut Io) -> i32 {
    1
}
