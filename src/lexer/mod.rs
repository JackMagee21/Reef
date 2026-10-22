//! Turns a line of text into tokens: words (following POSIX quoting rules)
//! and operators, each with its span.
//!
//! `$` forms and heredoc bodies come in later slices.

mod token;

use std::fmt;
use std::iter::Peekable;
use std::str::CharIndices;

pub use token::{Op, Span, Token, TokenKind};

use crate::word::{Word, WordPart};

#[derive(Debug, PartialEq)]
pub enum LexError {
    /// The input stopped in the middle of something, e.g. an unclosed quote.
    /// Piped mode reads another line instead of reporting it.
    Incomplete(&'static str),
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::Incomplete(what) => write!(f, "unexpected end of input: {what}"),
        }
    }
}

pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    let mut tokens = Vec::new();
    // The word being built and the byte where it started.
    // `Some` as soon as anything (even `''`) starts it.
    let mut cur: Option<(usize, Word)> = None;
    let mut chars = src.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        match c {
            // Unquoted blanks end the current word. `\n` is an operator, below.
            ' ' | '\t' | '\r' => end_word(&mut tokens, &mut cur, i),

            // `#` starts a comment only at the start of a word: `echo a#b` prints `a#b`.
            // The comment stops before the newline, which is still a token.
            '#' if cur.is_none() => while chars.next_if(|&(_, c)| c != '\n').is_some() {},

            // Single quotes: everything up to the next `'` is literal, no exceptions
            '\'' => {
                let mut text = String::new();
                loop {
                    match chars.next() {
                        Some((_, '\'')) => break,
                        Some((_, c)) => text.push(c),
                        None => return Err(LexError::Incomplete("unclosed '")),
                    }
                }
                word(&mut cur, i).push_str(&text, true);
            }

            // Double quotes: literal except that `\` escapes $ ` " \ and newline
            '"' => {
                let mut text = String::new();
                loop {
                    match chars.next() {
                        Some((_, '"')) => break,
                        Some((_, '\\')) => match chars.peek() {
                            Some(&(_, n @ ('$' | '`' | '"' | '\\'))) => {
                                chars.next();
                                text.push(n);
                            }
                            // backslash-newline is a line continuation: both disappear
                            Some((_, '\n')) => {
                                chars.next();
                            }
                            // any other backslash stays as-is: "a\b" is a\b
                            _ => text.push('\\'),
                        },
                        Some((_, c)) => text.push(c),
                        None => return Err(LexError::Incomplete("unclosed \"")),
                    }
                }
                word(&mut cur, i).push_str(&text, true);
            }

            // Unquoted backslash: the next character is literal
            '\\' => match chars.next() {
                // line continuation, but with nothing after it the next line is still to come
                // ponytail: so `-c 'echo a \<newline>'` is an error where bash prints `a`; split the cases if it matters
                Some((_, '\n')) if chars.peek().is_none() => {
                    return Err(LexError::Incomplete("line continuation"));
                }
                Some((_, '\n')) => {}
                Some((_, n)) => word(&mut cur, i).push_char(n, true),
                // a lone trailing backslash stays literal, like `bash -c 'echo \'`
                None => word(&mut cur, i).push_char('\\', false),
            },

            ';' | '&' | '|' | '(' | ')' | '<' | '>' | '\n' => {
                let op = read_op(c, &mut chars);
                // `2>file`: unquoted digits right before a redirect name a file descriptor
                if op.is_redirect()
                    && let Some((start, w)) = &cur
                    && let Some(n) = io_number(w)
                {
                    let span = Span {
                        start: *start,
                        end: i,
                    };
                    tokens.push(Token {
                        kind: TokenKind::IoNumber(n),
                        span,
                    });
                    cur = None;
                }
                end_word(&mut tokens, &mut cur, i);
                let end = chars.peek().map_or(src.len(), |&(j, _)| j);
                let span = Span { start: i, end };
                tokens.push(Token {
                    kind: TokenKind::Op(op),
                    span,
                });
            }

            c => word(&mut cur, i).push_char(c, false),
        }
    }

    end_word(&mut tokens, &mut cur, src.len());
    Ok(tokens)
}

/// The word being built, starting one at byte `i` if there isn't one yet.
fn word(cur: &mut Option<(usize, Word)>, i: usize) -> &mut Word {
    &mut cur.get_or_insert_with(|| (i, Word::default())).1
}

/// Push the word being built (if any) as a token ending at byte `end`.
fn end_word(tokens: &mut Vec<Token>, cur: &mut Option<(usize, Word)>, end: usize) {
    if let Some((start, w)) = cur.take() {
        let span = Span { start, end };
        tokens.push(Token {
            kind: TokenKind::Word(w),
            span,
        });
    }
}

/// The longest operator starting with `c`: `>>` is one operator, not two `>`.
fn read_op(c: char, chars: &mut Peekable<CharIndices>) -> Op {
    let mut next_is = |want: char| chars.next_if(|&(_, n)| n == want).is_some();
    match c {
        '\n' => Op::Newline,
        ';' if next_is(';') => Op::DSemi,
        ';' => Op::Semi,
        '&' if next_is('&') => Op::AndIf,
        '&' => Op::Amp,
        '|' if next_is('|') => Op::OrIf,
        '|' => Op::Pipe,
        '(' => Op::LParen,
        ')' => Op::RParen,
        '<' if next_is('<') => {
            if next_is('-') {
                Op::DLessDash
            } else {
                Op::DLess
            }
        }
        '<' if next_is('&') => Op::LessAnd,
        '<' if next_is('>') => Op::LessGreat,
        '<' => Op::Less,
        '>' if next_is('>') => Op::DGreat,
        '>' if next_is('&') => Op::GreatAnd,
        '>' if next_is('|') => Op::Clobber,
        '>' => Op::Great,
        _ => unreachable!("lex only calls read_op on operator characters"),
    }
}

/// `2` in `2>file`: one unquoted part, all ASCII digits. `'2'>file` and `a2>file` are words.
fn io_number(w: &Word) -> Option<u32> {
    match w.parts.as_slice() {
        [
            WordPart::Literal {
                text,
                quoted: false,
            },
        ] if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => text.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Just the words, as plain text. Newlines are skipped; any other operator fails the test.
    fn plain(src: &str) -> Vec<String> {
        lex(src)
            .unwrap()
            .into_iter()
            .filter_map(|t| match t.kind {
                TokenKind::Word(w) => Some(w.to_plain()),
                TokenKind::Op(Op::Newline) => None,
                other => panic!("unexpected {other:?} in {src:?}"),
            })
            .collect()
    }

    /// Token kinds only, with words simplified to their plain text.
    #[derive(Debug, PartialEq)]
    enum K {
        W(String),
        O(Op),
        N(u32),
    }

    fn kinds(src: &str) -> Vec<K> {
        lex(src)
            .unwrap()
            .into_iter()
            .map(|t| match t.kind {
                TokenKind::Word(w) => K::W(w.to_plain()),
                TokenKind::Op(op) => K::O(op),
                TokenKind::IoNumber(n) => K::N(n),
            })
            .collect()
    }

    fn w(s: &str) -> K {
        K::W(s.into())
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
        let tokens = lex("a'b'c").unwrap();
        let [
            Token {
                kind: TokenKind::Word(word),
                ..
            },
        ] = tokens.as_slice()
        else {
            panic!("{tokens:?}");
        };
        assert_eq!(
            word.parts,
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

    #[test]
    fn operators_end_words() {
        use Op::*;
        assert_eq!(kinds("a;b"), [w("a"), K::O(Semi), w("b")]);
        assert_eq!(
            kinds("a&&b||c|d&"),
            [
                w("a"),
                K::O(AndIf),
                w("b"),
                K::O(OrIf),
                w("c"),
                K::O(Pipe),
                w("d"),
                K::O(Amp)
            ]
        );
        assert_eq!(kinds("(a)"), [K::O(LParen), w("a"), K::O(RParen)]);
    }

    #[test]
    fn operators_take_the_longest_match() {
        use Op::*;
        let ops = |src| {
            kinds(src)
                .into_iter()
                .filter(|k| matches!(k, K::O(_)))
                .collect::<Vec<_>>()
        };
        assert_eq!(ops(";;"), [K::O(DSemi)]);
        assert_eq!(ops("a>>b"), [K::O(DGreat)]);
        assert_eq!(ops("a<<-b"), [K::O(DLessDash)]);
        assert_eq!(ops("a<<b"), [K::O(DLess)]);
        assert_eq!(ops("a<&b>&c"), [K::O(LessAnd), K::O(GreatAnd)]);
        assert_eq!(ops("a<>b>|c"), [K::O(LessGreat), K::O(Clobber)]);
        assert_eq!(ops("a>>>b"), [K::O(DGreat), K::O(Great)]);
        assert_eq!(ops("a|||b"), [K::O(OrIf), K::O(Pipe)]);
    }

    #[test]
    fn quoted_or_escaped_operators_are_text() {
        assert_eq!(
            plain(r#"echo 'a;b' "c|d" e\&f g\>h"#),
            ["echo", "a;b", "c|d", "e&f", "g>h"]
        );
    }

    #[test]
    fn newline_is_a_token_and_ends_a_comment() {
        assert_eq!(kinds("a\nb"), [w("a"), K::O(Op::Newline), w("b")]);
        assert_eq!(kinds("a # note\nb"), [w("a"), K::O(Op::Newline), w("b")]);
        assert_eq!(kinds("a;#b"), [w("a"), K::O(Op::Semi)]);
        // inside quotes a newline is just text
        assert_eq!(kinds("'a\nb'"), [w("a\nb")]);
    }

    #[test]
    fn digits_right_before_a_redirect_are_io_numbers() {
        use Op::*;
        assert_eq!(kinds("2>x"), [K::N(2), K::O(Great), w("x")]);
        assert_eq!(kinds("2>&1"), [K::N(2), K::O(GreatAnd), w("1")]);
        assert_eq!(kinds("10<x"), [K::N(10), K::O(Less), w("x")]);
        // not io numbers: a space, quotes, letters, or not a redirect
        assert_eq!(kinds("2 >x"), [w("2"), K::O(Great), w("x")]);
        assert_eq!(kinds("'2'>x"), [w("2"), K::O(Great), w("x")]);
        assert_eq!(kinds("a2>x"), [w("a2"), K::O(Great), w("x")]);
        assert_eq!(kinds("2|x"), [w("2"), K::O(Pipe), w("x")]);
    }

    #[test]
    fn spans_are_byte_ranges_into_the_input() {
        let src = "echo  'a b' >>f é;";
        let spans: Vec<&str> = lex(src)
            .unwrap()
            .iter()
            .map(|t| &src[t.span.start..t.span.end])
            .collect();
        assert_eq!(spans, ["echo", "'a b'", ">>", "f", "é", ";"]);
        let io: Vec<&str> = lex("2>&1")
            .unwrap()
            .iter()
            .map(|t| &"2>&1"[t.span.start..t.span.end])
            .collect();
        assert_eq!(io, ["2", ">&", "1"]);
    }
}
