//! Turns a line of text into words, following POSIX quoting rules.
//!
//! Knows only words, quotes, backslashes and comments so far. Operators
//! (`|`, `&&`, `;`, `>`), `$` forms and spans come in later slices.

use std::fmt;

use crate::word::Word;

#[derive(Debug, PartialEq)]
pub enum LexError {
    /// The input stopped in the middle of something, e.g. an unclosed quote.
    /// Later the REPL will read another line instead of reporting it.
    Incomplete(&'static str),
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::Incomplete(what) => write!(f, "unexpected end of input: {what}"),
        }
    }
}

pub fn lex(src: &str) -> Result<Vec<Word>, LexError> {
    let mut words = Vec::new();
    // The word being built. `Some` as soon as anything (even `''`) starts it.
    let mut cur: Option<Word> = None;
    let mut chars = src.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // Unquoted whitespace ends the current word
            ' ' | '\t' | '\n' | '\r' => {
                if let Some(w) = cur.take() {
                    words.push(w);
                }
            }

            // `#` starts a comment only at the start of a word: `echo a#b` prints `a#b`
            '#' if cur.is_none() => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }

            // Single quotes: everything up to the next `'` is literal, no exceptions
            '\'' => {
                let mut text = String::new();
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => text.push(c),
                        None => return Err(LexError::Incomplete("unclosed '")),
                    }
                }
                cur.get_or_insert_with(Word::default).push_str(&text, true);
            }

            // Double quotes: literal except that `\` escapes $ ` " \ and newline
            '"' => {
                let mut text = String::new();
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.peek() {
                            Some(&n @ ('$' | '`' | '"' | '\\')) => {
                                chars.next();
                                text.push(n);
                            }
                            // backslash-newline is a line continuation: both disappear
                            Some('\n') => {
                                chars.next();
                            }
                            // any other backslash stays as-is: "a\b" is a\b
                            _ => text.push('\\'),
                        },
                        Some(c) => text.push(c),
                        None => return Err(LexError::Incomplete("unclosed \"")),
                    }
                }
                cur.get_or_insert_with(Word::default).push_str(&text, true);
            }

            // Unquoted backslash: the next character is literal
            '\\' => match chars.next() {
                // line continuation, but with nothing after it the next line is still to come
                // ponytail: so `-c 'echo a \<newline>'` is an error where bash prints `a`; split the cases if it matters
                Some('\n') if chars.peek().is_none() => {
                    return Err(LexError::Incomplete("line continuation"));
                }
                Some('\n') => {}
                Some(n) => cur.get_or_insert_with(Word::default).push_char(n, true),
                // a lone trailing backslash stays literal, like `bash -c 'echo \'`
                None => cur.get_or_insert_with(Word::default).push_char('\\', false),
            },

            c => cur.get_or_insert_with(Word::default).push_char(c, false),
        }
    }

    if let Some(w) = cur {
        words.push(w);
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::word::WordPart;

    fn plain(src: &str) -> Vec<String> {
        lex(src).unwrap().iter().map(Word::to_plain).collect()
    }

    fn lit(text: &str, quoted: bool) -> WordPart {
        WordPart::Literal {
            text: text.into(),
            quoted,
        }
    }

    #[test]
    fn splits_on_unquoted_whitespace() {
        assert_eq!(plain("  echo   a\tb  "), ["echo", "a", "b"]);
        assert_eq!(plain(""), Vec::<String>::new());
    }

    #[test]
    fn adjacent_pieces_join_into_one_word_and_keep_their_quoting() {
        let words = lex("a'b'c").unwrap();
        assert_eq!(words.len(), 1);
        assert_eq!(
            words[0].parts,
            [lit("a", false), lit("b", true), lit("c", false)]
        );
        assert_eq!(plain(r#""echo hhe"bb"""#), ["echo hhebb"]);
    }

    #[test]
    fn empty_quotes_make_an_empty_word() {
        assert_eq!(plain("''"), [""]);
        assert_eq!(plain(r#"a "" b"#), ["a", "", "b"]);
    }

    #[test]
    fn single_quotes_are_fully_literal() {
        assert_eq!(plain(r#"'a "b" \c $d'"#), [r#"a "b" \c $d"#]);
    }

    #[test]
    fn double_quote_backslash_only_escapes_special_chars() {
        assert_eq!(plain(r#""a\"b""#), [r#"a"b"#]);
        assert_eq!(plain(r#""a\\b""#), [r"a\b"]);
        assert_eq!(plain(r#""a\$b""#), ["a$b"]);
        assert_eq!(plain(r#""a\nb""#), [r"a\nb"]);
        assert_eq!(plain(r#""it's""#), ["it's"]);
    }

    #[test]
    fn unquoted_backslash_escapes_next_char() {
        assert_eq!(plain(r"a\ b"), ["a b"]);
        assert_eq!(plain(r"\'x"), ["'x"]);
        assert_eq!(plain("a\\\nb"), ["ab"]);
        assert_eq!(plain(r"echo \"), ["echo", r"\"]);
    }

    #[test]
    fn comments_only_start_a_word() {
        assert_eq!(plain("echo hi # rest"), ["echo", "hi"]);
        assert_eq!(plain("echo a#b"), ["echo", "a#b"]);
        assert_eq!(plain("# all\necho x"), ["echo", "x"]);
        assert_eq!(plain("echo '#x'"), ["echo", "#x"]);
    }

    #[test]
    fn unclosed_quotes_are_incomplete() {
        assert_eq!(lex("echo 'a"), Err(LexError::Incomplete("unclosed '")));
        assert_eq!(lex("echo \"a"), Err(LexError::Incomplete("unclosed \"")));
        assert_eq!(
            lex("echo a \\\n"),
            Err(LexError::Incomplete("line continuation"))
        );
    }

    #[test]
    fn non_ascii_text_is_kept_intact() {
        assert_eq!(plain("échø 'ñ ü'"), ["échø", "ñ ü"]);
    }
}
