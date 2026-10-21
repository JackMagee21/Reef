use std::path::{Path, PathBuf};
use std::process::ExitStatus;

/// Status Windows gives a process that was ended by Ctrl-C.
const STATUS_CONTROL_C_EXIT: u32 = 0xC000013A;

/// Find a command the way cmd.exe does: try each `PATHEXT` extension.
/// A name containing a path separator is resolved against `cwd` instead of `PATH`.
pub fn find_command(name: &str, path_var: &str, cwd: &Path) -> Option<PathBuf> {
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| e.to_ascii_lowercase())
        .collect();

    let try_exts = |base: PathBuf| -> Option<PathBuf> {
        // `git.exe` typed in full: use it as-is
        let has_ext = base
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| exts.contains(&format!(".{}", e.to_ascii_lowercase())));
        if has_ext && base.is_file() {
            return Some(base);
        }
        exts.iter()
            .map(|ext| {
                let mut p = base.clone().into_os_string();
                p.push(ext);
                PathBuf::from(p)
            })
            .find(|p| p.is_file())
    };

    if name.contains(['/', '\\']) {
        return try_exts(cwd.join(name));
    }
    std::env::split_paths(path_var).find_map(|dir| try_exts(dir.join(name)))
}

/// Turn a child's exit status into a shell status (`$?`).
pub fn exit_status(status: ExitStatus) -> i32 {
    match status.code() {
        Some(c) if c as u32 == STATUS_CONTROL_C_EXIT => 130,
        Some(c) => c,
        None => 1,
    }
}

/// Environment variable names are case-insensitive on Windows: `Path` and `PATH` are one variable.
pub fn var_key(name: &str) -> String {
    name.to_ascii_uppercase()
}
