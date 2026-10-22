//! A shell word: one argument as the user typed it, before expansion.
//!
//! `"echo hhe"bb""` is ONE word made of three parts. Each part remembers whether
//! it was quoted, because later `$x` vs `"$x"` and `*` vs `"*"` behave differently.

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Word {
    pub parts: Vec<WordPart>,
}

/// Later steps add `Param` ($x), `CommandSub` ($(...)), `Arith` ($((...))) and `Tilde`.
#[derive(Debug, Clone, PartialEq)]
pub enum WordPart {
    Literal { text: String, quoted: bool },
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

    // shortcut: joins the parts as-is; the expander replaces this once $x and globs exist
    pub fn to_plain(&self) -> String {
        self.parts
            .iter()
            .map(|p| match p {
                WordPart::Literal { text, .. } => text.as_str(),
            })
            .collect()
    }
}
