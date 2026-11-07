//! A shell word: one argument as the user typed it, before expansion.
//!
//! `"echo hhe"bb""` is ONE word made of three parts. Each part remembers whether
//! it was quoted, because later `$x` vs `"$x"` and `*` vs `"*"` behave differently.

use crate::lexer::Span;
use crate::shell::is_valid_name;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Word {
    pub parts: Vec<WordPart>,
    /// Where it was typed, for errors that point at it. Empty (0..0) for words
    /// Reef makes itself, like the value part of `x=$y`.
    pub span: Span,
}

/// Later steps add `Arith` ($((...))).
#[derive(Debug, Clone, PartialEq)]
pub enum WordPart {
    Literal {
        text: String,
        quoted: bool,
    },
    /// `$x`, `${x}` or `$?`. `name` is whatever was between the braces;
    /// `exec` refuses anything that isn't a plain name or `?` before running.
    Param {
        name: String,
        braced: bool,
        quoted: bool,
    },
    /// `$(...)`: the commands inside, as text. Parsed (by the same parser as
    /// everything else) when checked and when run.
    CommandSub {
        src: String,
        quoted: bool,
        /// Where `src` starts in the input it was typed in, so an error inside
        /// can point at the right character of the whole line.
        offset: usize,
    },
}

impl Word {
    /// Append text, merging into the last part when its quoting matches.
    /// An empty quoted piece (`''`) still adds a part, so `echo ''` gets one empty argument.
    pub fn push_str(&mut self, s: &str, quoted: bool) {
        if let Some(WordPart::Literal { text, quoted: q }) = self.parts.last_mut()
            && *q == quoted
        {
            text.push_str(s);
            return;
        }
        self.parts.push(WordPart::Literal {
            text: s.to_string(),
            quoted,
        });
    }

    pub fn push_char(&mut self, c: char, quoted: bool) {
        self.push_str(c.encode_utf8(&mut [0; 4]), quoted);
    }

    /// The text, if the word is one unquoted piece: `if` and `2` yes, `'if'`, `"2"` and `a'b'` no.
    /// For words that are only special when written plainly: reserved words, `2` in `2>x`.
    pub fn as_unquoted(&self) -> Option<&str> {
        match self.parts.as_slice() {
            [
                WordPart::Literal {
                    text,
                    quoted: false,
                },
            ] => Some(text),
            _ => None,
        }
    }

    /// The word written back without quotes, for messages: `a'b'$x` is `ab$x`.
    /// Running a command uses `expand` instead.
    pub fn joined_text(&self) -> String {
        self.parts
            .iter()
            .map(|p| match p {
                WordPart::Literal { text, .. } => text.clone(),
                WordPart::Param {
                    name, braced: true, ..
                } => format!("${{{name}}}"),
                WordPart::Param { name, .. } => format!("${name}"),
                WordPart::CommandSub { src, .. } => format!("$({src})"),
            })
            .collect()
    }

    /// Has a `$(...)` in it, so expanding it runs commands.
    pub fn has_command_sub(&self) -> bool {
        self.parts
            .iter()
            .any(|p| matches!(p, WordPart::CommandSub { .. }))
    }

    /// `name=value` written plainly at the start: the name and the value as a
    /// word of its own. `x=$y` and `x="a b"` count; `"x=1"` and `1x=2` don't.
    pub fn assignment(&self) -> Option<(String, Word)> {
        let (
            WordPart::Literal {
                text,
                quoted: false,
            },
            rest,
        ) = self.parts.split_first()?
        else {
            return None;
        };
        let (name, value) = text.split_once('=')?;
        if !is_valid_name(name) {
            return None;
        }
        let mut word = Word::default();
        if !value.is_empty() {
            word.push_str(value, false);
        }
        word.parts.extend_from_slice(rest);
        Some((name.to_string(), word))
    }
}

#[cfg(test)]
mod tests {
    use crate::lexer::{TokenKind, lex};

    fn assignment(src: &str) -> Option<(String, String)> {
        let TokenKind::Word(w) = &lex(src).unwrap()[0].kind else {
            panic!("{src:?}")
        };
        w.assignment()
            .map(|(name, value)| (name, value.joined_text()))
    }

    #[test]
    fn assignments_need_a_plain_valid_name_first() {
        let yes = |n: &str, v: &str| Some((n.to_string(), v.to_string()));
        assert_eq!(assignment("x=1"), yes("x", "1"));
        assert_eq!(assignment("x="), yes("x", ""));
        assert_eq!(assignment("x=a=b"), yes("x", "a=b"));
        assert_eq!(assignment("x=$y"), yes("x", "$y"));
        assert_eq!(assignment("x='a b'c"), yes("x", "a bc"));
        assert_eq!(assignment("'x'=1"), None);
        assert_eq!(assignment("\"x=1\""), None);
        assert_eq!(assignment("1x=1"), None);
        assert_eq!(assignment("=1"), None);
        assert_eq!(assignment("x"), None);
    }
}
