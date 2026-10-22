//! `cd`: change the shell's directory.

use crate::platform;
use crate::shell::Shell;
use crate::streams::{Io, describe_error};

/// `cd [dir | -]`
///
/// - no argument: the first set variable in `platform::HOME_VARS`
/// - `-`: `$OLDPWD`, and prints where it went
/// - otherwise `dir`, relative to `sh.cwd`
///
/// Physical, not logical: `..` means the real parent, even through a symlink.
/// On success `PWD` and `OLDPWD` are updated and exported.
pub fn cd(sh: &mut Shell, argv: &[String], io: &mut Io) -> i32 {
    let fail = |io: &mut Io, msg: &str| {
        io.error(format_args!("cd: {msg}"));
        1
    };

    let target = match argv.get(1..).unwrap_or(&[]) {
        [] => match platform::HOME_VARS.iter().find_map(|v| sh.var(v)) {
            Some(home) => home.to_string(),
            None => return fail(io, "HOME not set"),
        },
        [dash] if dash == "-" => match sh.var("OLDPWD") {
            Some(old) => old.to_string(),
            None => return fail(io, "OLDPWD not set"),
        },
        [dir] => dir.clone(),
        _ => return fail(io, "too many arguments"),
    };

    let new = match std::fs::canonicalize(platform::resolve(sh.cwd(), &target)) {
        Ok(p) if p.is_dir() => platform::clean_path(p),
        Ok(_) => return fail(io, &format!("{target}: Not a directory")),
        Err(e) => return fail(io, &format!("{target}: {}", describe_error(&e))),
    };

    sh.set_cwd(new);

    // `cd -` prints where it went
    if argv.get(1).is_some_and(|a| a == "-") {
        return super::print(io, "cd", &format!("{}\n", sh.cwd().display()));
    }
    0
}
