//! `reef --update`: reinstall the newest Reef release from GitHub with cargo,
//! into the same place the running one was installed. `reef --check-update`:
//! say whether that would change anything. `reef --rollback`: go back to the
//! version the last update replaced.
//!
//! Only releases (tags like `v0.4.0`) are ever installed, never whatever was
//! pushed last, and never an older version than the one running. Builds use
//! `--locked`, so the dependencies are exactly the ones the release was tested
//! with. Nothing is sent anywhere but GitHub, and only to ask what's there.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::platform;
use crate::streams::describe_error;

const REPO: &str = "https://github.com/JackMagee21/Reef";

/// A release: `v0.4.0` is (0, 4, 0).
type Version = (u64, u64, u64);

/// The version this reef was built as.
fn running() -> Version {
    parse_version(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0))
}

/// Returns Reef's exit status. Cargo itself prints the progress.
///
/// The reef being replaced is kept as `reef.previous` for `--rollback`, but
/// only once the new one is in: a failed update leaves the old backup alone.
pub fn run() -> i32 {
    let fail = |message: &str| fail("--update", message);
    let (exe, root) = match installed() {
        Ok(found) => found,
        Err(message) => return fail(&message),
    };
    let (version, commit) = match newest_release() {
        Ok(release) => release,
        Err(message) => return fail(&message),
    };
    let current = env!("CARGO_PKG_VERSION");
    // A release older than this one is never "newer", even if it's the newest tag
    if version < running() {
        println!(
            "reef {current} is newer than the newest release ({}); nothing to do",
            show(version)
        );
        return 0;
    }
    // Already have it: stop here, so the backup still holds the version before
    if version == running() && installed_commit(&crates_list(&root)) == Some(commit) {
        println!("reef {current} is already the newest release");
        return 0;
    }

    // A copy of this reef, made before anything changes
    let backup = backup_path(&exe);
    let pending = exe.with_file_name(format!(
        "reef.previous.pending{}",
        std::env::consts::EXE_SUFFIX
    ));
    if let Err(e) = std::fs::copy(&exe, &pending) {
        return fail(&format!(
            "can't keep a copy of this reef to roll back to ({}), so not updating",
            describe_error(&e)
        ));
    }

    let moved = match platform::move_aside_running_exe(&exe) {
        Ok(moved) => moved,
        Err(e) => {
            let _ = std::fs::remove_file(&pending);
            return fail(&format!(
                "can't replace {}: {}",
                exe.display(),
                describe_error(&e)
            ));
        }
    };
    let result = Command::new("cargo")
        .args([
            "install",
            "--locked",
            // This code already decided to install, e.g. after a rollback,
            // when cargo's own list says the release is in place
            "--force",
            "--git",
            REPO,
            "--tag",
            &tag(version),
            "--root",
        ])
        .arg(&root)
        // fail instead of asking for a password if the repo is ever unreachable
        .env("GIT_TERMINAL_PROMPT", "0")
        .status();
    let code = match result {
        Ok(s) if s.success() => {
            if let Err(e) = std::fs::rename(&pending, &backup) {
                eprintln!(
                    "reef: --update: updated, but couldn't keep the old version for --rollback: {}",
                    describe_error(&e)
                );
            }
            println!(
                "updated to reef {}; `reef --rollback` goes back to {current}",
                show(version)
            );
            return 0;
        }
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
    let _ = std::fs::remove_file(&pending);
    code
}

/// `reef --rollback`: swap this reef with the one the last update replaced.
/// Swapping, not deleting, so running it again goes forward again.
pub fn rollback() -> i32 {
    let fail = |message: &str| fail("--rollback", message);
    let (exe, _) = match installed() {
        Ok(found) => found,
        Err(message) => return fail(&message),
    };
    let backup = backup_path(&exe);
    if exe == backup {
        return fail(
            "this is the previous version already; run `reef --rollback` with the normal `reef`",
        );
    }
    if !backup.is_file() {
        return fail(
            "there's no previous version to go back to; `reef --update` keeps one each time it installs a new release",
        );
    }
    // Three renames through a spare name, each undone if a later one fails
    let spare = exe.with_file_name(format!("reef.swap{}", std::env::consts::EXE_SUFFIX));
    let rename = |from: &Path, to: &Path| {
        std::fs::rename(from, to)
            .map_err(|e| format!("can't move {}: {}", from.display(), describe_error(&e)))
    };
    if let Err(message) = rename(&exe, &spare) {
        return fail(&message);
    }
    if let Err(message) = rename(&backup, &exe) {
        let _ = std::fs::rename(&spare, &exe);
        return fail(&message);
    }
    if let Err(message) = rename(&spare, &backup) {
        // The previous reef is in place; this one is still safe under the spare name
        return fail(&format!(
            "{message}; this version is kept as {}",
            spare.display()
        ));
    }
    println!(
        "switched back from reef {} to the previous version; open a new reef to use it. \
         `reef --rollback` again switches forward",
        env!("CARGO_PKG_VERSION")
    );
    0
}

/// Where `--update` keeps the reef it replaced: `reef.previous` next to `reef`.
fn backup_path(exe: &Path) -> PathBuf {
    exe.with_file_name(format!("reef.previous{}", std::env::consts::EXE_SUFFIX))
}

/// `reef --check-update`: is there a release newer than this reef? Status 0
/// when up to date, 1 when there's an update or it couldn't tell (and says why).
pub fn check() -> i32 {
    let fail = |message: &str| fail("--check-update", message);
    let root = match installed() {
        Ok((_, root)) => root,
        Err(message) => return fail(&message),
    };
    let (version, commit) = match newest_release() {
        Ok(release) => release,
        Err(message) => return fail(&message),
    };
    let current = env!("CARGO_PKG_VERSION");
    // The version decides; the commit only matters between builds of one version
    // (cargo's list can be stale after `--rollback`, so it can't decide alone)
    let up_to_date = match version.cmp(&running()) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Greater => false,
        // Built from a local folder: no commit to compare, and the same version
        std::cmp::Ordering::Equal => {
            installed_commit(&crates_list(&root)).is_none_or(|have| have == commit)
        }
    };
    if up_to_date {
        println!(
            "reef {current} is up to date (newest release: {})",
            show(version)
        );
        0
    } else {
        println!(
            "reef {} is available (you have {current}); run `reef --update`",
            show(version)
        );
        1
    }
}

/// Cargo's list of what it installed under `root`; empty if there isn't one.
fn crates_list(root: &Path) -> String {
    std::fs::read_to_string(root.join(".crates.toml")).unwrap_or_default()
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
/// `"reef 0.4.0 (git+https://github.com/JackMagee21/Reef?tag=v0.4.0#08d23aa...)" = ["reef"]`.
/// Older installs have no `?tag=...` part.
fn installed_commit(crates: &str) -> Option<String> {
    let marker = format!("(git+{REPO}");
    crates
        .lines()
        .filter(|line| line.starts_with("\"reef "))
        .find_map(|line| {
            let source = line.split_once(&marker)?.1.split_once(')')?.0;
            // Exactly this repo: `#commit` or `?tag=...#commit`, not `.../Reef-fork#...`
            if !source.starts_with(['#', '?']) {
                return None;
            }
            Some(source.split_once('#')?.1.to_string())
        })
}

/// The newest release on GitHub and its commit, from `git ls-remote` (a few
/// hundred bytes, no download).
fn newest_release() -> Result<(Version, String), String> {
    let out = Command::new("git")
        .args(["ls-remote", "--tags", REPO])
        // fail instead of asking for a password if the repo is ever unreachable
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("can't run git ({})", describe_error(&e)))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        return Err(format!("can't reach {REPO}: {}", why.trim()));
    }
    newest_in(&String::from_utf8_lossy(&out.stdout))
        .ok_or_else(|| format!("{REPO} has no releases yet (tags like v0.4.0)"))
}

/// The highest `vX.Y.Z` tag in `git ls-remote --tags` output, and its commit.
///
/// Anything else (`v1.0-beta`, `latest`, a name that's really an option like
/// `--x`) is ignored, so only a plain release number ever reaches cargo.
/// An annotated tag is listed twice: as itself, and as `v0.4.0^{}` with the
/// commit it points at, which is the one cargo records.
fn newest_in(listing: &str) -> Option<(Version, String)> {
    let mut best: Option<(Version, String, bool)> = None;
    for line in listing.lines() {
        let Some((commit, name)) = line.split_once('\t') else {
            continue;
        };
        let Some(name) = name.strip_prefix("refs/tags/v") else {
            continue;
        };
        let (name, peeled) = match name.strip_suffix("^{}") {
            Some(name) => (name, true),
            None => (name, false),
        };
        let Some(version) = parse_version(name) else {
            continue;
        };
        let better = match &best {
            None => true,
            Some((v, _, was_peeled)) => version > *v || version == *v && peeled && !was_peeled,
        };
        if better {
            best = Some((version, commit.to_string(), peeled));
        }
    }
    best.map(|(version, commit, _)| (version, commit))
}

/// `0.4.0` as (0, 4, 0). Exactly three plain numbers, nothing else.
fn parse_version(text: &str) -> Option<Version> {
    let mut parts = text
        .split('.')
        .map(|p| match p.bytes().all(|b| b.is_ascii_digit()) {
            true => p.parse::<u64>().ok(),
            false => None,
        });
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

fn show((major, minor, patch): Version) -> String {
    format!("{major}.{minor}.{patch}")
}

fn tag(version: Version) -> String {
    format!("v{}", show(version))
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
             \"reef 0.4.0 (git+{REPO}?tag=v0.4.0#08d23aaf25506610)\" = [\"reef\"]\n"
        );
        assert_eq!(
            installed_commit(&crates).as_deref(),
            Some("08d23aaf25506610")
        );
        // before releases: straight from the branch
        let branch = format!("\"reef 0.3.0 (git+{REPO}#62183b5)\" = [\"reef\"]");
        assert_eq!(installed_commit(&branch).as_deref(), Some("62183b5"));
        // a fork whose name starts the same isn't this repo
        let fork = format!("\"reef 0.3.0 (git+{REPO}-fork#62183b5)\" = [\"reef\"]");
        assert_eq!(installed_commit(&fork), None);
        // installed from a local folder: no commit to compare
        let local = "\"reef 0.3.0 (path+file:///home/jmt/Documents/Reef)\" = [\"reef\"]";
        assert_eq!(installed_commit(local), None);
        assert_eq!(installed_commit(""), None);
    }

    #[test]
    fn the_newest_release_is_the_highest_plain_version() {
        let listing = "\
            aaa\trefs/tags/v0.3.0\n\
            bbb\trefs/tags/v0.10.0\n\
            ccc\trefs/tags/v0.9.9\n\
            ddd\trefs/tags/v2.0.0-beta\n\
            eee\trefs/tags/latest\n\
            fff\trefs/tags/v--upload-pack=x\n\
            ggg\trefs/heads/v9.9.9\n";
        // 10 beats 9 as a number, not as text; the rest aren't releases
        assert_eq!(newest_in(listing), Some(((0, 10, 0), "bbb".into())));
        assert_eq!(newest_in(""), None);
    }

    #[test]
    fn an_annotated_tag_gives_the_commit_it_points_at() {
        // `^{}` is the commit; the plain line is the tag object, in either order
        let listing = "tag1\trefs/tags/v0.4.0\ncommit1\trefs/tags/v0.4.0^{}\n";
        assert_eq!(newest_in(listing), Some(((0, 4, 0), "commit1".into())));
        let listing = "commit1\trefs/tags/v0.4.0^{}\ntag1\trefs/tags/v0.4.0\n";
        assert_eq!(newest_in(listing), Some(((0, 4, 0), "commit1".into())));
    }

    #[test]
    fn versions_are_exactly_three_numbers() {
        assert_eq!(parse_version("0.4.0"), Some((0, 4, 0)));
        assert_eq!(parse_version("10.20.30"), Some((10, 20, 30)));
        for bad in ["0.4", "0.4.0.1", "0.4.0-rc1", "+1.0.0", "a.b.c", "", "0..0"] {
            assert_eq!(parse_version(bad), None, "{bad:?}");
        }
        assert_eq!(tag((0, 4, 0)), "v0.4.0");
    }
}
