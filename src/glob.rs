//! Filename patterns: `*.txt`, `?`, `[a-z]`, matched against what's on disk.
//!
//! A pattern arrives with quoted characters already escaped (`"*"` as `\*`),
//! so `\` always means "this next character is literal".

use std::path::{Path, PathBuf};

use crate::platform;

/// Does `text` contain an unescaped `*`, `?` or `[`? Only then is it a pattern.
pub fn is_pattern(text: &str) -> bool {
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '*' | '?' | '[' => return true,
            _ => {}
        }
    }
    false
}

/// The paths `pattern` matches, sorted, written the way the pattern was:
/// `src/*.rs` gives `src/main.rs`, `/tmp/*` gives `/tmp/x`. Relative to `cwd`.
/// Empty when nothing matches; the caller then keeps the word as typed.
///
/// Names starting with `.` only match a pattern that starts with `.`, like sh.
// shortcut: no `[[:alpha:]]` classes, and names that aren't UTF-8 never match
pub fn expand(pattern: &str, cwd: &Path) -> Vec<String> {
    // (what to print, where it is on disk)
    let mut found = match pattern.strip_prefix('/') {
        // `/` on Unix, the current drive's root on Windows
        Some(_) => vec![(String::from("/"), platform::resolve(cwd, "/"))],
        None => vec![(String::new(), cwd.to_path_buf())],
    };
    let parts: Vec<&str> = pattern.trim_start_matches('/').split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        found = found
            .into_iter()
            .flat_map(|(shown, path)| step(&shown, &path, part, last))
            .collect();
    }
    let mut paths: Vec<String> = found.into_iter().map(|(shown, _)| shown).collect();
    paths.sort();
    paths
}

/// Where one `/`-separated piece of the pattern leads from the folder `path`.
fn step(shown: &str, path: &Path, part: &str, last: bool) -> Vec<(String, PathBuf)> {
    // `sub/` (a trailing or doubled `/`): only folders
    if part.is_empty() {
        return match path.is_dir() {
            true => vec![(format!("{shown}/"), path.to_path_buf())],
            false => vec![],
        };
    }
    if !is_pattern(part) {
        let name = unescape(part);
        let next = path.join(&name);
        // A missing folder in the middle fails at `read_dir`; only the end must exist
        if last && next.symlink_metadata().is_err() {
            return vec![];
        }
        return vec![(join(shown, &name), next)];
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return vec![];
    };
    let pattern: Vec<char> = part.chars().collect();
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') && !part.starts_with('.') || !matches(&pattern, &name) {
                return None;
            }
            let next = path.join(&name);
            (last || next.is_dir()).then(|| (join(shown, &name), next))
        })
        .collect()
}

/// `shown` and `name` with a `/` between them, unless `shown` already ends in one.
fn join(shown: &str, name: &str) -> String {
    if shown.is_empty() || shown.ends_with('/') {
        format!("{shown}{name}")
    } else {
        format!("{shown}/{name}")
    }
}

/// The literal text of a piece with no pattern in it: `a\*b` is `a*b`.
fn unescape(part: &str) -> String {
    let mut out = String::new();
    let mut chars = part.chars();
    while let Some(c) = chars.next() {
        out.push(if c == '\\' {
            chars.next().unwrap_or('\\')
        } else {
            c
        });
    }
    out
}

/// Does the file name `name` match `pattern`? `*` is any run of characters,
/// `?` any one, `[abc]`, `[a-z]` and `[!a-z]` one from a set, `\x` a literal `x`.
pub fn matches(pattern: &[char], name: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let (mut p, mut n) = (0, 0);
    // After a mismatch, the last `*` swallows one more character and we retry
    // from there: (pattern index after the `*`, name index it has reached)
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if pattern.get(p) == Some(&'*') {
            p += 1;
            star = Some((p, n));
            continue;
        }
        if p < pattern.len()
            && let (true, width) = one(&pattern[p..], name[n])
        {
            p += width;
            n += 1;
            continue;
        }
        let Some((after_star, reached)) = star else {
            return false;
        };
        p = after_star;
        n = reached + 1;
        star = Some((after_star, n));
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// Does the start of `pattern` match the character `c`? Also returns how many
/// pattern characters that took (`?` is 1, `[a-z]` is 5).
fn one(pattern: &[char], c: char) -> (bool, usize) {
    match pattern {
        ['?', ..] => (true, 1),
        ['[', ..] => set(pattern, c).unwrap_or((same('[', c), 1)),
        ['\\', escaped, ..] => (same(*escaped, c), 2),
        [first, ..] => (same(*first, c), 1),
        [] => (false, 0),
    }
}

/// `[abc]`, `[a-z]` or `[!abc]` at the start of `pattern`. `None` without a
/// closing `]`; then the `[` is just a character. A `]` right after the `[`
/// (or `[!`) is part of the set, and so is a `-` at either end.
fn set(pattern: &[char], c: char) -> Option<(bool, usize)> {
    let negated = matches!(pattern.get(1), Some('!' | '^'));
    let start = if negated { 2 } else { 1 };
    let mut i = start;
    let mut found = false;
    loop {
        let first = *pattern.get(i)?;
        if first == ']' && i > start {
            return Some((found != negated, i + 1));
        }
        match pattern.get(i + 1..i + 3) {
            Some(['-', last]) if *last != ']' => {
                found |= in_range(first, *last, c);
                i += 3;
            }
            _ => {
                found |= same(first, c);
                i += 1;
            }
        }
    }
}

fn same(a: char, b: char) -> bool {
    a == b || platform::NAMES_IGNORE_CASE && a.to_lowercase().eq(b.to_lowercase())
}

fn in_range(low: char, high: char, c: char) -> bool {
    let range = low..=high;
    range.contains(&c)
        || platform::NAMES_IGNORE_CASE
            && (range.contains(&c.to_ascii_lowercase()) || range.contains(&c.to_ascii_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, name: &str) -> bool {
        matches(&pattern.chars().collect::<Vec<_>>(), name)
    }

    #[test]
    fn star_and_question_mark() {
        assert!(m("*.rs", "main.rs"));
        assert!(!m("*.rs", "main.rsx"));
        assert!(m("*", ""));
        assert!(m("a*b*c", "aXXbYbc"));
        assert!(!m("a*b*c", "aXXbY"));
        assert!(m("?.rs", "c.rs"));
        assert!(!m("?.rs", ".rs"));
        assert!(m("**x", "abx"));
        assert!(m("é*", "école"));
    }

    #[test]
    fn sets() {
        assert!(m("[abc].txt", "b.txt"));
        assert!(!m("[abc].txt", "d.txt"));
        assert!(m("[a-c]", "b"));
        assert!(m("[!a-c]", "d"));
        assert!(!m("[^a-c]", "a"));
        assert!(m("[]]", "]"));
        assert!(m("[a-]", "-"));
        // no closing `]`: a plain `[`
        assert!(m("[", "["));
        assert!(m("a[b", "a[b"));
    }

    #[test]
    fn escapes_are_literal() {
        assert!(m(r"\*", "*"));
        assert!(!m(r"\*", "x"));
        assert!(m(r"a\?", "a?"));
        assert!(!is_pattern(r"\*.txt"));
        assert!(is_pattern("*.txt"));
        assert_eq!(unescape(r"a\*b\\"), r"a*b\");
    }
}
