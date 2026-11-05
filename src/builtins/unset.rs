//! `unset`: remove variables.

use crate::shell::{Shell, is_valid_name};
use crate::streams::Io;

/// `unset [-v] [name...]`: removes each variable, exported or not. A name
/// that was never set isn't an error. `-v` (variables, the default) is
/// accepted; there are no functions yet, so `-f` isn't.
///
/// An invalid name is reported and skipped, and the status is 1.
pub fn unset(sh: &mut Shell, argv: &[String], io: &mut Io) -> i32 {
    let mut args = argv.get(1..).unwrap_or(&[]);
    if let [first, rest @ ..] = args
        && first == "-v"
    {
        args = rest;
    }

    let mut failed = false;
    for name in args {
        if is_valid_name(name) {
            sh.unset_var(name);
        } else {
            io.error(format_args!("unset: `{name}': not a valid identifier"));
            failed = true;
        }
    }
    if failed { 1 } else { 0 }
}
