//! `echo`: print the arguments.

use crate::io::Io;
use crate::shell::Shell;

/// `echo [-n] [args...]`: the arguments joined by single spaces, then a
/// newline unless `-n` is given.
///
/// No escape sequences (`\n`, `\t`) and no other options, on purpose:
/// shells disagree about them, which is why scripts use `printf` instead.
pub fn echo(_sh: &mut Shell, argv: &[String], io: &mut Io) -> i32 {
    let args = argv.get(1..).unwrap_or(&[]);
    let (newline, args) = match args {
        [first, rest @ ..] if first == "-n" => (false, rest),
        _ => (true, args),
    };

    let mut out = args.join(" ");
    if newline {
        out.push('\n');
    }

    super::print(io, "echo", &out)
}
