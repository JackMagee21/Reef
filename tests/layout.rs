//! Rules about the source tree itself.
//!
//! A Rust test rather than a CI grep, so `cargo test` (and `make check`)
//! enforce it on every OS without needing grep installed.

use std::path::Path;

/// OS-specific code (`#[cfg(windows)]`, `cfg!(unix)`, `#[cfg_attr(windows, ...)]`)
/// is only allowed inside `src/platform/`.
#[test]
fn os_specific_code_only_in_platform() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut bad = vec![];
    check_dir(&src, &src.join("platform"), &mut bad);
    assert!(
        bad.is_empty(),
        "move these into src/platform/:\n{}",
        bad.join("\n")
    );
}

fn check_dir(dir: &Path, allowed: &Path, bad: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path == allowed {
            continue;
        }
        if path.is_dir() {
            check_dir(&path, allowed, bad);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).unwrap();
            for (i, line) in text.lines().enumerate() {
                if is_os_cfg(line) {
                    bad.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                }
            }
        }
    }
}

fn is_os_cfg(line: &str) -> bool {
    let has_cfg = ["cfg(", "cfg!(", "cfg_attr("]
        .iter()
        .any(|c| line.contains(c));
    let has_os = line
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|w| OS_WORDS.contains(&w));
    has_cfg && has_os
}

/// Words that make a `cfg` OS-specific: `target_os = "linux"` slips past a check for just windows/unix.
const OS_WORDS: &[&str] = &[
    "windows",
    "unix",
    "target_os",
    "target_family",
    "target_env",
    "target_vendor",
];

#[test]
fn detects_every_cfg_form() {
    assert!(is_os_cfg("#[cfg(windows)]"));
    assert!(is_os_cfg("if cfg!(unix) {"));
    assert!(is_os_cfg("#[cfg_attr(windows, allow(x))]"));
    assert!(is_os_cfg(r#"#[cfg(target_os = "windows")]"#));
    assert!(is_os_cfg(r#"#[cfg(target_os = "linux")]"#));
    assert!(is_os_cfg(r#"if cfg!(target_family = "wasm") {"#));
    assert!(is_os_cfg(r#"#[cfg(target_env = "msvc")]"#));
    assert!(!is_os_cfg("#[cfg(test)]"));
    assert!(!is_os_cfg("// windows only"));
}
