use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

/// Find an executable file named `name` on `PATH`.
/// A name containing `/` is resolved against `cwd` instead of `PATH`.
pub fn find_command(name: &str, path_var: &str, cwd: &Path) -> Option<PathBuf> {
    let is_exec = |p: &PathBuf| {
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };

    if name.contains('/') {
        return Some(cwd.join(name)).filter(is_exec);
    }
    std::env::split_paths(path_var)
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
