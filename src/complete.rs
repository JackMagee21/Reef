//! Tab completion: what could finish the word under the cursor.
//!
//! So far only command names: the first word of a command, from the builtins
//! and the programs on `PATH`. File names come next.

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

/// Suggestions for the word that ends at byte `pos` of `line`.
// shortcut: quotes aren't understood, so `"my fi` completes from `fi`
pub fn complete(line: &str, pos: usize, sh: &Shell) -> Vec<Suggestion> {
    let Some(before) = line.get(..pos) else {
        return Vec::new();
    };
    let start = before
        .char_indices()
        .rev()
        .find(|&(_, c)| c.is_whitespace() || BREAKS.contains(&c))
        .map_or(0, |(i, c)| i + c.len_utf8());
    let word = &before[start..];

    // shortcut: an empty word would list every program (thousands); offer nothing
    if word.is_empty() || word.contains('/') || !starts_command(&before[..start]) {
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
        .map(|value| Suggestion {
            value,
            span: Span::new(start, pos),
            append_whitespace: true,
            ..Suggestion::default()
        })
        .collect()
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

    /// The suggestions for `line` with the cursor at its end.
    fn names(line: &str) -> Vec<String> {
        complete(line, line.len(), &Shell::new())
            .into_iter()
            .map(|s| s.value)
            .collect()
    }

    #[test]
    fn the_first_word_completes_to_builtins_and_programs() {
        let found = names("expo");
        assert!(found.contains(&"export".to_string()), "{found:?}");
        // `cargo` runs these tests, so it's on PATH
        assert!(names("carg").contains(&"cargo".to_string()));
        assert!(names("expo").iter().all(|n| n.starts_with("expo")));
    }

    #[test]
    fn a_command_name_follows_operators() {
        for line in ["ls | expo", "a && expo", "a || expo", "a; expo", "(expo", "{ expo", "! expo", "a\nexpo"] {
            assert!(names(line).contains(&"export".to_string()), "{line:?}");
        }
    }

    #[test]
    fn other_words_are_not_command_names() {
        for line in ["echo expo", "ls > expo", "", "echo ", "./expo", "x=1 expo"] {
            assert_eq!(names(line), Vec::<String>::new(), "{line:?}");
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
    fn names_are_sorted_and_listed_once() {
        let found = names("e");
        let mut sorted = found.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(found, sorted);
    }
}
