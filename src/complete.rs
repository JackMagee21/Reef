//! Tab completion: what could finish the word under the cursor.
//!
//! The first word of a command completes to a builtin or a program on `PATH`;
//! any other word, or one with a `/` in it, to a file or folder name.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use reedline::{Completer, CompletionResult, Span, Suggestion};

use crate::builtins;
use crate::platform;
use crate::shell::Shell;

/// What the line editor asks when Tab is pressed.
///
/// It holds a copy of the shell, refreshed before every prompt, because
/// completing depends on the directory and `PATH`, which `cd` and `export`
/// change. `Arc<Mutex<_>>` because the line editor requires `Send`.
pub struct TabCompleter(pub Arc<Mutex<Shell>>);

impl Completer for TabCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> CompletionResult {
        // A panic elsewhere while holding the lock can't leave a copy half-written
        let sh = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        CompletionResult::fresh(complete(line, pos, &sh))
    }
}

/// Characters that end a word, besides whitespace.
const BREAKS: &[char] = &['|', '&', ';', '(', ')', '{', '}', '<', '>'];

/// Characters a file name needs a `\` before, to stay one plain word.
const SPECIAL: &[char] = &[
    ' ', '\t', '\\', '\'', '"', '$', '`', '|', '&', ';', '(', ')', '<', '>', '*', '?', '[', ']',
    '#', '{', '}', '!',
];

/// Suggestions for the word that ends at byte `pos` of `line`.
// shortcut: quotes aren't understood, so `"my fi` completes from `fi`
pub fn complete(line: &str, pos: usize, sh: &Shell) -> Vec<Suggestion> {
    let Some(before) = line.get(..pos) else {
        return Vec::new();
    };
    let start = word_start(before);
    let word = &before[start..];
    let span = Span::new(start, pos);
    if word.contains('/') || !starts_command(&before[..start]) {
        return files(word, sh, span);
    }
    // shortcut: an empty word would list every program (thousands); offer nothing
    if word.is_empty() {
        return Vec::new();
    }
    let mut names: Vec<String> = builtins::names()
        .filter(|name| name.starts_with(word))
        .map(String::from)
        .chain(platform::commands_starting_with(word, sh))
        .collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|value| suggestion(value, span, true))
        .collect()
}

/// Where the last word of `before` starts: after the last space or operator
/// character that isn't escaped, so `cat My\ Fi` gives `My\ Fi`.
fn word_start(before: &str) -> usize {
    let mut start = 0;
    let mut chars = before.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == '\\' {
            chars.next();
        } else if c.is_whitespace() || BREAKS.contains(&c) {
            start = i + c.len_utf8();
        }
    }
    start
}

/// Files and folders that could finish `word`: `src/pa` looks in `src/` for
/// names starting with `pa`. Folders end in `/` and no space, so Tab can go on
/// into them. What's already typed stays as typed, `~/` included.
fn files(word: &str, sh: &Shell, span: Span) -> Vec<Suggestion> {
    let (dir, prefix) = match word.rfind('/') {
        Some(i) => word.split_at(i + 1),
        None => ("", word),
    };
    let prefix = unescape(prefix);
    let unescaped = unescape(dir);
    let home = sh.first_var(platform::HOME_VARS);
    let look_in = match (unescaped.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => Path::new(home).join(rest),
        _ => platform::resolve(sh.cwd(), &unescaped),
    };
    let Ok(entries) = std::fs::read_dir(look_in) else {
        return Vec::new();
    };
    let wanted = |name: &str| match platform::NAMES_IGNORE_CASE {
        true => name.to_lowercase().starts_with(&prefix.to_lowercase()),
        false => name.starts_with(&prefix),
    };
    let mut found: Vec<(String, bool)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            // Hidden names only once their `.` is typed, like `*` in a glob
            let shown = !name.starts_with('.') || prefix.starts_with('.');
            (shown && wanted(&name)).then(|| (name, entry.path().is_dir()))
        })
        .collect();
    found.sort();
    found
        .into_iter()
        .map(|(name, is_dir)| {
            let value = format!("{dir}{}{}", escape(&name), if is_dir { "/" } else { "" });
            suggestion(value, span, !is_dir)
        })
        .collect()
}

fn suggestion(value: String, span: Span, append_whitespace: bool) -> Suggestion {
    Suggestion {
        value,
        span,
        append_whitespace,
        ..Suggestion::default()
    }
}

/// `My File` as one word: `My\ File`.
fn escape(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if SPECIAL.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// What a typed word means: `My\ Fi` is `My Fi`.
fn unescape(typed: &str) -> String {
    let mut out = String::new();
    let mut chars = typed.chars();
    while let Some(c) = chars.next() {
        out.push(if c == '\\' {
            chars.next().unwrap_or(c)
        } else {
            c
        });
    }
    out
}

/// Is the word after `before` a command name? It is at the start of the
/// line, and after `|`, `||`, `&&`, `;`, `&`, `(`, `{`, `!` or a newline.
fn starts_command(before: &str) -> bool {
    let trimmed = before.trim_end_matches([' ', '\t']);
    trimmed.is_empty() || trimmed.ends_with(['|', '&', ';', '(', '{', '!', '\n'])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The suggestions for `line` with the cursor at its end, from inside `dir`.
    fn names_in(dir: &Path, line: &str) -> Vec<String> {
        let mut sh = Shell::new();
        sh.set_cwd(dir.to_path_buf());
        complete(line, line.len(), &sh)
            .into_iter()
            .map(|s| s.value)
            .collect()
    }

    fn names(line: &str) -> Vec<String> {
        names_in(&std::env::temp_dir(), line)
    }

    /// A fresh folder holding `paths`; a path ending in `/` is made a folder.
    fn folder(test: &str, paths: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("reef_complete_{test}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for p in paths {
            let path = dir.join(p);
            if p.ends_with('/') {
                std::fs::create_dir_all(path).unwrap();
            } else {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, "").unwrap();
            }
        }
        dir
    }

    #[test]
    fn the_first_word_completes_to_builtins_and_programs() {
        let found = names("expo");
        assert!(found.contains(&"export".to_string()), "{found:?}");
        // `cargo` runs these tests, so it's on PATH
        assert!(names("carg").contains(&"cargo".to_string()));
        assert!(names("expo").iter().all(|n| n.starts_with("expo")));
        // an empty first word would list thousands
        assert!(names("").is_empty());
    }

    #[test]
    fn a_command_name_follows_operators() {
        for line in [
            "ls | expo",
            "a && expo",
            "a || expo",
            "a; expo",
            "(expo",
            "{ expo",
            "! expo",
            "a\nexpo",
        ] {
            assert!(names(line).contains(&"export".to_string()), "{line:?}");
        }
    }

    #[test]
    fn only_the_word_before_the_cursor_is_replaced() {
        let line = "echo a; expo rest";
        let found = complete(line, 12, &Shell::new());
        let export = found.iter().find(|s| s.value == "export").unwrap();
        assert_eq!(export.span, Span::new(8, 12));
        assert!(export.append_whitespace);
    }

    #[test]
    fn command_names_are_sorted_and_listed_once() {
        let found = names("e");
        let mut sorted = found.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(found, sorted);
    }

    #[test]
    fn other_words_complete_to_files_and_folders() {
        let dir = folder(
            "files",
            &["notes.txt", "notes.md", "src/", "src/main.rs", "export"],
        );
        assert_eq!(names_in(&dir, "cat no"), ["notes.md", "notes.txt"]);
        // folders end in `/`, so Tab can carry on into them
        assert_eq!(names_in(&dir, "cd s"), ["src/"]);
        assert_eq!(names_in(&dir, "cat src/m"), ["src/main.rs"]);
        assert_eq!(names_in(&dir, "echo hi > no"), ["notes.md", "notes.txt"]);
        // an empty word lists the folder
        assert_eq!(names_in(&dir, "ls ").len(), 4);
        // a first word with `/` in it is a file to run
        assert_eq!(names_in(&dir, "./src/ma"), ["./src/main.rs"]);
        // not a command position: files, not the builtin `export`
        assert_eq!(names_in(&dir, "echo expo"), ["export"]);
    }

    #[test]
    fn folders_get_no_space_and_files_do() {
        let dir = folder("space", &["a.txt", "adir/"]);
        let found = complete("ls a", 4, &{
            let mut sh = Shell::new();
            sh.set_cwd(dir.clone());
            sh
        });
        let spaced: Vec<(&str, bool)> = found
            .iter()
            .map(|s| (s.value.as_str(), s.append_whitespace))
            .collect();
        assert_eq!(spaced, [("a.txt", true), ("adir/", false)]);
        assert_eq!(found[0].span, Span::new(3, 4));
    }

    #[test]
    fn hidden_files_need_their_dot_typed() {
        let dir = folder("hidden", &[".secret", "seen"]);
        assert_eq!(names_in(&dir, "ls "), ["seen"]);
        assert_eq!(names_in(&dir, "ls ."), [".secret"]);
    }

    #[test]
    fn names_with_spaces_are_escaped_both_ways() {
        let dir = folder("escape", &["My File.txt", "it's"]);
        assert_eq!(names_in(&dir, "cat My"), [r"My\ File.txt"]);
        // what's typed so far may already be escaped
        assert_eq!(names_in(&dir, r"cat My\ F"), [r"My\ File.txt"]);
        assert_eq!(names_in(&dir, "cat it"), [r"it\'s"]);
    }

    #[test]
    fn tilde_means_home_and_stays_as_typed() {
        let dir = folder("home", &["docs/"]);
        let mut sh = Shell::new();
        sh.set_var("HOME", dir.display().to_string(), true);
        let found: Vec<String> = complete("cd ~/d", 6, &sh)
            .into_iter()
            .map(|s| s.value)
            .collect();
        assert_eq!(found, ["~/docs/"]);
    }
}
