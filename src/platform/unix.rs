use std::fs::File;
use std::os::fd::AsFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use crate::shell::Shell;

/// Find an executable file named `name` on `PATH`.
/// A name containing `/` is resolved against `sh.cwd` instead of `PATH`.
pub fn find_command(name: &str, sh: &Shell) -> Option<PathBuf> {
    let is_exec = |p: &PathBuf| {
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };

    if name.contains('/') {
        return Some(sh.cwd.join(name)).filter(is_exec);
    }
    std::env::split_paths(sh.var("PATH").unwrap_or_default())
        .map(|dir| dir.join(name))
        .find(is_exec)
}

/// Turn a child's exit status into a shell status (`$?`): killed by signal N becomes 128 + N.
pub fn exit_status(status: ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| status.signal().map(|s| 128 + s))
        .unwrap_or(1)
}

/// Environment variable names are case-sensitive on Unix.
pub fn var_key(name: &str) -> String {
    name.to_string()
}

/// Where `cd` with no argument goes, first one set wins.
pub const HOME_VARS: &[&str] = &["HOME"];

/// Unix paths from `canonicalize` are already clean.
pub fn clean_path(path: PathBuf) -> PathBuf {
    path
}

/// `target` relative to `cwd`. Unix has no drive-relative paths.
pub fn resolve(cwd: &Path, target: &str) -> PathBuf {
    cwd.join(target)
}

/// Reef's stdin as an unbuffered file, so reading it never reads ahead.
pub fn stdin_file() -> std::io::Result<File> {
    Ok(File::from(std::io::stdin().as_fd().try_clone_to_owned()?))
}
