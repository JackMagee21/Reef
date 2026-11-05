//! `reef --update`: reinstall the newest Reef from GitHub with cargo, into
//! the same place the running one was installed. `reef --check-update`: say
//! whether that would change anything.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::platform;
use crate::streams::describe_error;

const REPO: &str = "https://github.com/JackMagee21/Reef";

/// Returns Reef's exit status. Cargo itself prints the progress, and skips
/// the build when the installed copy is already the newest commit.
// shortcut: builds the latest commit on GitHub's default branch, not a release; switch to tags once releases exist
pub fn run() -> i32 {
    let fail = |message: &str| fail("--update", message);
    let (exe, root) = match installed() {
        Ok(found) => found,
        Err(message) => return fail(&message),
    };

    let moved = match platform::move_aside_running_exe(&exe) {
        Ok(moved) => moved,
        Err(e) => {
            return fail(&format!(
                "can't replace {}: {}",
                exe.display(),
                describe_error(&e)
            ));
        }
    };
    let result = Command::new("cargo")
        .args(["install", "--git", REPO, "--root"])
        .arg(&root)
        .status();
    let code = match result {
        Ok(s) if s.success() => return 0,
        Ok(s) => platform::exit_status(s),
        Err(e) => fail(&format!(
            "can't run cargo ({}); install Rust from https://rustup.rs",
            describe_error(&e)
        )),
    };
    // The update failed: put the old reef back if it was moved and nothing replaced it
    if let Some(old) = moved
        && !exe.exists()
        && let Err(e) = std::fs::rename(&old, &exe)
    {
        eprintln!("reef: --update: couldn't restore {}: {e}", exe.display());
    }
    code
}

/// `reef --check-update`: compares the commit cargo installed with the newest
/// one on GitHub. Status 0 when up to date, 1 when there's an update or it
/// couldn't tell (and says why).
pub fn check() -> i32 {
    let fail = |message: &str| fail("--check-update", message);
    let root = match installed() {
        Ok((_, root)) => root,
        Err(message) => return fail(&message),
    };
    let crates = std::fs::read_to_string(root.join(".crates.toml")).unwrap_or_default();
    let Some(have) = installed_commit(&crates) else {
        return fail(
            "this reef wasn't installed from GitHub, so there's no commit to compare; `reef --update` installs the newest",
        );
    };
    let newest = match newest_commit() {
        Ok(commit) => commit,
        Err(message) => return fail(&message),
    };
    let short = |commit: &str| commit.get(..7).unwrap_or(commit).to_string();
    if have == newest {
        println!(
            "reef {} is up to date ({})",
            env!("CARGO_PKG_VERSION"),
            short(&have)
        );
        0
    } else {
        println!(
            "a newer reef is available ({} → {}); run `reef --update`",
            short(&have),
            short(&newest)
        );
        1
    }
}

/// The running reef and the folder `cargo install --root` put it in (it lives in `<root>/bin`).
fn installed() -> Result<(PathBuf, PathBuf), String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("can't find reef itself: {}", describe_error(&e)))?;
    let root = exe
        .parent()
        .filter(|dir| dir.file_name() == Some("bin".as_ref()))
        .and_then(Path::parent)
        .ok_or_else(|| {
            format!(
                "{} wasn't installed with `cargo install`; install it with:\n  \
                 cargo install --git {REPO} --root ~/.local",
                exe.display()
            )
        })?
        .to_path_buf();
    Ok((exe, root))
}

/// The commit in cargo's list of installed programs (`<root>/.crates.toml`), from a line like
/// `"reef 0.3.0 (git+https://github.com/JackMagee21/Reef#08d23aa...)" = ["reef"]`.
fn installed_commit(crates: &str) -> Option<String> {
    let marker = format!("(git+{REPO}#");
    crates
        .lines()
        .filter(|line| line.starts_with("\"reef "))
        .find_map(|line| Some(line.split_once(&marker)?.1.split_once(')')?.0.to_string()))
}

/// The newest commit on GitHub, from `git ls-remote` (a few hundred bytes, no download).
fn newest_commit() -> Result<String, String> {
    let out = Command::new("git")
        .args(["ls-remote", REPO, "HEAD"])
        // fail instead of asking for a password if the repo is ever unreachable
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("can't run git ({})", describe_error(&e)))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        return Err(format!("can't reach {REPO}: {}", why.trim()));
    }
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or_else(|| format!("{REPO} has no commits"))
}

fn fail(flag: &str, message: &str) -> i32 {
    eprintln!("reef: {flag}: {message}");
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_installed_commit() {
        let crates = format!(
            "[v1]\n\
             \"other 1.0.0 (registry+https://x)\" = [\"other\"]\n\
             \"reef 0.3.0 (git+{REPO}#08d23aaf25506610)\" = [\"reef\"]\n"
        );
        assert_eq!(
            installed_commit(&crates).as_deref(),
            Some("08d23aaf25506610")
        );
        // installed from a local folder: no commit to compare
        let local = "\"reef 0.3.0 (path+file:///home/jmt/Documents/Reef)\" = [\"reef\"]";
        assert_eq!(installed_commit(local), None);
        assert_eq!(installed_commit(""), None);
    }
}
