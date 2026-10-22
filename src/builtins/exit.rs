//! `exit`: stop the shell.

use crate::io::Io;
use crate::shell::Shell;

/// `exit [n]`: exit with `n` (mod 256), or with `$?` when no `n` is given.
///
/// Only sets `sh.exit_requested`; whatever runs commands stops after this one.
/// Calling `process::exit` here would skip the final flush, and would kill the
/// whole shell from inside a subshell.
pub fn exit(sh: &mut Shell, argv: &[String], io: &mut Io) -> i32 {
    let code = match argv.get(1) {
        None => sh.last_status,
        Some(arg) => match arg.parse::<i64>() {
            // A bad number still exits, like bash
            Err(_) => {
                let _ = io.write_err(
                    format!("reef: exit: {arg}: numeric argument required\n").as_bytes(),
                );
                sh.exit_requested = true;
                return 2;
            }
            // Too many arguments is an error that does NOT exit, like bash
            Ok(_) if argv.len() > 2 => {
                let _ = io.write_err(b"reef: exit: too many arguments\n");
                return 1;
            }
            Ok(n) => n.rem_euclid(256) as i32,
        },
    };
    sh.exit_requested = true;
    code
}
