//! `cd`: change the shell's directory.

use crate::io::Io;
use crate::platform;
use crate::shell::Shell;

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
        let _ = io.write_err(format!("reef: cd: {msg}\n").as_bytes());
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

    let new = match std::fs::canonicalize(platform::resolve(&sh.cwd, &target)) {
        Ok(p) if p.is_dir() => platform::clean_path(p),
        Ok(_) => return fail(io, &format!("{target}: Not a directory")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return fail(io, &format!("{target}: No such file or directory"));
        }
        Err(e) => return fail(io, &format!("{target}: {e}")),
    };

    let old = std::mem::replace(&mut sh.cwd, new);
    sh.set_var("OLDPWD", old.display().to_string(), true);
    sh.set_var("PWD", sh.cwd.display().to_string(), true);

    if argv.get(1).is_some_and(|a| a == "-")
        && let Err(e) = io.write_out(format!("{}\n", sh.cwd.display()).as_bytes())
    {
        return fail(io, &format!("write error: {e}"));
    }
    0
}
