//! `exit`: stop the shell.

use crate::shell::Shell;
use crate::status;
use crate::streams::Io;

/// `exit [n]`: exit with `n` (mod 256), or with `$?` when no `n` is given.
///
/// Only sets `sh.stop_requested`; whatever runs commands stops after this one.
/// Calling `process::exit` here would skip the final flush, and would kill the
/// whole shell from inside a subshell.
pub fn exit(sh: &mut Shell, argv: &[String], io: &mut Io) -> i32 {
    let code = match argv.get(1) {
        None => sh.last_status,
        Some(arg) => match arg.parse::<i64>() {
            // A bad number still exits, like bash
            Err(_) => {
                io.error(format_args!("exit: {arg}: numeric argument required"));
                sh.stop_requested = true;
                return status::SYNTAX_ERROR;
            }
            // Too many arguments is an error that does NOT exit, like bash
            Ok(_) if argv.len() > 2 => {
                io.error("exit: too many arguments");
                return 1;
            }
            Ok(n) => n.rem_euclid(256) as i32,
        },
    };
    sh.stop_requested = true;
    code
}
