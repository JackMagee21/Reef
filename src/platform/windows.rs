use std::fs::File;
use std::os::windows::io::AsHandle;
use std::path::{Component, Path, PathBuf};
use std::process::ExitStatus;

use crate::shell::Shell;

/// Find a command the way cmd.exe does: try each `PATHEXT` extension.
/// A name containing a path separator is resolved against `sh.cwd` instead of `PATH`.
pub fn find_command(name: &str, sh: &Shell) -> Option<PathBuf> {
    let exts: Vec<String> = sh
        .var("PATHEXT")
        .unwrap_or(".COM;.EXE;.BAT;.CMD")
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
        return try_exts(resolve(&sh.cwd, name));
    }
    // A relative PATH entry is relative to the shell's directory, not Reef's start directory
    std::env::split_paths(sh.var("PATH").unwrap_or_default())
        .filter(|dir| !dir.as_os_str().is_empty())
        .find_map(|dir| try_exts(sh.cwd.join(dir).join(name)))
}

/// Turn a child's exit code into a shell status (`$?`), always 0-255.
///
/// Windows exit codes are 32 bits. Crashes report an NTSTATUS code like
/// 0xC0000005, which passed through as-is looks to Reef's own parent like
/// Reef crashed. Known crash codes become 128 + the matching Unix signal,
/// like on Linux; anything else keeps its low 8 bits, like `exit(300)` does on Unix.
// ponytail: only the common crash codes are mapped; unknown NTSTATUS codes fall back to the low byte
pub fn exit_status(status: ExitStatus) -> i32 {
    let Some(code) = status.code() else { return 1 };
    match code as u32 {
        0xC000013A => 130,              // Ctrl-C: SIGINT
        0xC0000005 | 0xC00000FD => 139, // access violation, stack overflow: SIGSEGV
        0xC000001D => 132,              // illegal instruction: SIGILL
        0xC0000094 => 136,              // integer divide by zero: SIGFPE
        0xC0000409 => 134,              // fail-fast (abort): SIGABRT
        c => match c & 0xFF {
            // never turn a failure into success
            0 if c != 0 => 1,
            low => low as i32,
        },
    }
}

/// Environment variable names are case-insensitive on Windows: `Path` and `PATH` are one variable.
pub fn var_key(name: &str) -> String {
    name.to_ascii_uppercase()
}

/// Where `cd` with no argument goes, first one set wins. `HOME` is usually unset on Windows.
pub const HOME_VARS: &[&str] = &["HOME", "USERPROFILE"];

/// Drop the `\\?\` prefix `canonicalize` adds, so users see `C:\dir`, not `\\?\C:\dir`.
// ponytail: plain string strip; paths over 260 chars may need the prefix kept, revisit if anyone hits it
pub fn clean_path(path: PathBuf) -> PathBuf {
    match path.to_str() {
        Some(s) if s.starts_with(r"\\?\UNC\") => PathBuf::from(format!(r"\\{}", &s[8..])),
        Some(s) if s.starts_with(r"\\?\") => PathBuf::from(&s[4..]),
        _ => path,
    }
}

/// `target` relative to `cwd`.
///
/// `C:` and `C:dir` (a drive with no `\`) would otherwise resolve against the
/// process's own directory, which Reef never changes. Instead they use `cwd`
/// when it's on that drive, and the drive's root otherwise.
// ponytail: no per-drive directories like cmd.exe keeps; add them if switching drives back and forth matters
pub fn resolve(cwd: &Path, target: &str) -> PathBuf {
    let target = Path::new(target);
    let mut parts = target.components();
    match parts.next() {
        Some(Component::Prefix(drive)) if !target.has_root() => {
            let same_drive = matches!(cwd.components().next(),
                Some(Component::Prefix(p)) if p.as_os_str().eq_ignore_ascii_case(drive.as_os_str()));
            let base = if same_drive {
                cwd.to_path_buf()
            } else {
                // the drive's root, e.g. `D:\`
                let mut root = drive.as_os_str().to_os_string();
                root.push(r"\");
                PathBuf::from(root)
            };
            base.join(parts.as_path())
        }
        _ => cwd.join(target),
    }
}

/// Reef's stdin as an unbuffered file, so reading it never reads ahead.
pub fn stdin_file() -> std::io::Result<File> {
    Ok(File::from(
        std::io::stdin().as_handle().try_clone_to_owned()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::ExitStatusExt;

    fn status(code: u32) -> i32 {
        exit_status(ExitStatus::from_raw(code))
    }

    #[test]
    fn exit_codes_are_always_0_to_255() {
        assert_eq!(status(0), 0);
        assert_eq!(status(42), 42);
        assert_eq!(status(300), 44);
        assert_eq!(status(0xFFFF_FFFF), 255); // exit(-1)
        assert_eq!(status(256), 1);
        assert_eq!(status(0xC000013A), 130);
        assert_eq!(status(0xC0000005), 139);
    }

    #[test]
    fn drive_relative_paths_use_the_shells_dir() {
        let cwd = Path::new(r"C:\work\src");
        assert_eq!(resolve(cwd, "C:"), Path::new(r"C:\work\src"));
        assert_eq!(resolve(cwd, "c:lib"), Path::new(r"C:\work\src\lib"));
        assert_eq!(resolve(cwd, "D:"), Path::new(r"D:\"));
        assert_eq!(resolve(cwd, "D:x"), Path::new(r"D:\x"));
        assert_eq!(resolve(cwd, r"D:\x"), Path::new(r"D:\x"));
        assert_eq!(resolve(cwd, ".."), Path::new(r"C:\work\src\.."));
    }
}
