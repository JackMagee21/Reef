use std::fs::File;
use std::os::fd::AsFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use crate::shell::Shell;

/// Find an executable file named `name` on `PATH`.
///
/// A name containing `/` is resolved against `sh.cwd` and returned if it exists
/// at all, so a file without the execute bit fails to start (126, "Permission
/// denied") rather than being "not found" (127), like bash.
/// Empty and relative `PATH` entries are relative to `sh.cwd`, not Reef's start directory.
/// With `PATH` unset, `DEFAULT_PATH` is searched, never the current directory.
pub fn find_command(name: &str, sh: &Shell) -> Option<PathBuf> {
    if name.contains('/') {
        let path = sh.cwd().join(name);
        return path.exists().then_some(path);
    }
    let is_exec = |p: &PathBuf| {
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    std::env::split_paths(sh.var("PATH").unwrap_or(DEFAULT_PATH))
        .map(|dir| sh.cwd().join(dir).join(name))
        .find(is_exec)
}

/// Searched when `PATH` is unset. Not empty: an empty `PATH` means the current
/// directory, which would let any file there pose as `ls`.
const DEFAULT_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

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

/// Who's logged in, for the prompt (`jmt@link`); first one set wins.
pub const USER_VARS: &[&str] = &["USER", "LOGNAME"];

/// The machine's name, for the prompt (`jmt@link`).
// shortcut: Linux only (/proc); macOS falls back to `$HOSTNAME`, then `localhost`
pub fn host_name(sh: &Shell) -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .or_else(|| sh.var("HOSTNAME").map(str::to_string))
        .unwrap_or_else(|| "localhost".into())
}

/// Where Reef keeps its own files, like the history:
/// `$XDG_DATA_HOME/reef`, or `~/.local/share/reef`. `None` without a home.
pub fn data_dir(sh: &Shell) -> Option<PathBuf> {
    let base = match sh.var("XDG_DATA_HOME") {
        // The XDG spec says to ignore relative paths
        Some(dir) if Path::new(dir).is_absolute() => PathBuf::from(dir),
        _ => Path::new(sh.var("HOME").filter(|h| !h.is_empty())?).join(".local/share"),
    };
    Some(base.join("reef"))
}

/// Unix paths from `canonicalize` are already clean.
pub fn clean_path(path: PathBuf) -> PathBuf {
    path
}

/// `target` relative to `cwd`. Unix has no drive-relative paths.
pub fn resolve(cwd: &Path, target: &str) -> PathBuf {
    cwd.join(target)
}

/// The file a redirect like `> target` opens, relative to `cwd`.
pub fn redirect_path(cwd: &Path, target: &str) -> PathBuf {
    resolve(cwd, target)
}

/// Unix can replace a running program's file directly, so `--update` needs nothing moved.
pub fn move_aside_running_exe(_exe: &Path) -> std::io::Result<Option<PathBuf>> {
    Ok(None)
}

/// Reef's stdin as an unbuffered file, so reading it never reads ahead.
pub fn stdin_file() -> std::io::Result<File> {
    Ok(File::from(std::io::stdin().as_fd().try_clone_to_owned()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_follows_xdg_then_home() {
        let mut sh = Shell::new();
        sh.set_var("HOME", "/home/jmt".into(), false);
        sh.unset_var("XDG_DATA_HOME");
        assert_eq!(
            data_dir(&sh),
            Some(PathBuf::from("/home/jmt/.local/share/reef"))
        );
        sh.set_var("XDG_DATA_HOME", "/data".into(), false);
        assert_eq!(data_dir(&sh), Some(PathBuf::from("/data/reef")));
        sh.set_var("XDG_DATA_HOME", "relative".into(), false);
        assert_eq!(
            data_dir(&sh),
            Some(PathBuf::from("/home/jmt/.local/share/reef"))
        );
        sh.unset_var("XDG_DATA_HOME");
        sh.unset_var("HOME");
        assert_eq!(data_dir(&sh), None);
    }
}
