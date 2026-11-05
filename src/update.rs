//! `reef --update`: reinstall the newest Reef from GitHub with cargo, into
//! the same place the running one was installed.

use std::path::Path;
use std::process::Command;

use crate::platform;
use crate::streams::describe_error;

const REPO: &str = "https://github.com/JackMagee21/Reef";

/// Returns Reef's exit status. Cargo itself prints the progress, and skips
/// the build when the installed copy is already the newest commit.
// shortcut: builds the latest commit on GitHub's default branch, not a release; switch to tags once releases exist
pub fn run() -> i32 {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return fail(&format!("can't find reef itself: {}", describe_error(&e))),
    };
    // `cargo install --root R` puts programs in R/bin
    let Some(root) = exe
        .parent()
        .filter(|dir| dir.file_name() == Some("bin".as_ref()))
        .and_then(Path::parent)
    else {
        return fail(&format!(
            "{} wasn't installed with `cargo install`; install it with:\n  \
             cargo install --git {REPO} --root ~/.local",
            exe.display()
        ));
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
        .arg(root)
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

fn fail(message: &str) -> i32 {
    eprintln!("reef: --update: {message}");
    1
}
