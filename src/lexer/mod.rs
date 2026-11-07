//! Turns a line of text into tokens: words (following POSIX quoting rules)
//! and operators, each with its span.
//!
//! `$x`, `${x}` and `$?` become `WordPart::Param`, `$(...)` becomes
//! `WordPart::CommandSub`; `$((...))` and heredoc bodies come in later slices.

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
    // The word being built. `Some` as soon as anything (even `''`) starts it.
    let mut cur: Option<Pending> = None;
    let mut chars = src.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        match c {
            // Unquoted blanks end the current word. `\n` is an operator, below.
            ' ' | '\t' | '\r' => end_word(&mut tokens, &mut cur, i),

            // `#` starts a comment only at the start of a word: `echo a#b` prints `a#b`.
            // The comment stops before the newline, which is still a token.
            '#' if cur.is_none() => while chars.next_if(|&(_, c)| c != '\n').is_some() {},

            '\'' => {
                let text = read_single_quoted(&mut chars)?;
                word(&mut cur, i).push_str(&text, true);
            }

            '"' => read_double_quoted(&mut chars, word(&mut cur, i))?,

            '$' => read_dollar(&mut chars, word(&mut cur, i), false)?,

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
                    && let Some(pending) = &cur
                    && let Some(n) = io_number(&pending.word)
                {
                    let span = Span {
                        start: pending.start,
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

/// A word still being built, and the byte where it started (for its span).
struct Pending {
    start: usize,
    word: Word,
}

/// The word being built, starting one at byte `i` if there isn't one yet.
fn word(cur: &mut Option<Pending>, i: usize) -> &mut Word {
    &mut cur
        .get_or_insert_with(|| Pending {
            start: i,
            word: Word::default(),
        })
        .word
}

/// Push the word being built (if any) as a token ending at byte `end`.
fn end_word(tokens: &mut Vec<Token>, cur: &mut Option<Pending>, end: usize) {
    if let Some(Pending { start, mut word }) = cur.take() {
        let span = Span { start, end };
        word.span = span;
        tokens.push(Token {
            kind: TokenKind::Word(word),
            span,
        });
    }
}

/// The text after an opening `'`, up to the closing one. Everything in between
/// is literal, no exceptions.
fn read_single_quoted(chars: &mut Peekable<CharIndices>) -> Result<String, LexError> {
    let mut text = String::new();
    loop {
        match chars.next() {
            Some((_, '\'')) => return Ok(text),
            Some((_, c)) => text.push(c),
            None => return Err(LexError::Incomplete("unclosed '")),
        }
    }
}

/// The text after an opening `"`, up to the closing one, added to `word` as
/// quoted parts. Literal, except for `$x` forms, and `\` escaping `$`, `` ` ``,
/// `"`, `\` and newline.
fn read_double_quoted(chars: &mut Peekable<CharIndices>, word: &mut Word) -> Result<(), LexError> {
    // `""` is still a (quoted, empty) part, so `echo ""` gets one empty argument
    word.push_str("", true);
    loop {
        match chars.next() {
            Some((_, '"')) => return Ok(()),
            Some((_, '\\')) => match chars.peek() {
                Some(&(_, n @ ('$' | '`' | '"' | '\\'))) => {
                    chars.next();
                    word.push_char(n, true);
                }
                // backslash-newline is a line continuation: both disappear
                Some((_, '\n')) => {
                    chars.next();
                }
                // any other backslash stays as-is: "a\b" is a\b
                _ => word.push_char('\\', true),
            },
            Some((_, '$')) => read_dollar(chars, word, true)?,
            Some((_, c)) => word.push_char(c, true),
            None => return Err(LexError::Incomplete("unclosed \"")),
        }
    }
}

/// What follows a `$`, added to `word`: `name`, `{anything}` or `?` become a
/// `Param`, `(...)` a `CommandSub`; before anything else the `$` is just a
/// character (`echo $`, `a$ b`).
// shortcut: `$1`, `$#`, `$$`, `$@` stay literal; add them with positional arguments
fn read_dollar(
    chars: &mut Peekable<CharIndices>,
    word: &mut Word,
    quoted: bool,
) -> Result<(), LexError> {
    let mut param = |name: String, braced| {
        word.parts.push(WordPart::Param {
            name,
            braced,
            quoted,
        });
        Ok(())
    };
    match chars.peek() {
        Some((_, '(')) => {
            chars.next();
            let src = read_command_sub(chars)?;
            word.parts.push(WordPart::CommandSub { src, quoted });
            Ok(())
        }
        Some((_, '{')) => {
            chars.next();
            let mut name = String::new();
            loop {
                match chars.next() {
                    Some((_, '}')) => return param(name, true),
                    Some((_, c)) => name.push(c),
                    None => return Err(LexError::Incomplete("unclosed ${")),
                }
            }
        }
        Some((_, '?')) => {
            chars.next();
            param("?".into(), false)
        }
        Some(&(_, c)) if c.is_ascii_alphabetic() || c == '_' => {
            let mut name = String::new();
            while let Some((_, c)) = chars.next_if(|&(_, c)| c.is_ascii_alphanumeric() || c == '_')
            {
                name.push(c);
            }
            param(name, false)
        }
        _ => {
            word.push_char('$', quoted);
            Ok(())
        }
    }
}

/// The text of `$( ... )` after the `$(`, up to its matching `)`. Quotes, `\`,
/// nested `( )` and comments are stepped over, so `$(echo ")")` ends in the
/// right place. Parsing the text is left to the parser, later.
// shortcut: a `case` pattern's lone `)` would end it early; revisit with `case`
fn read_command_sub(chars: &mut Peekable<CharIndices>) -> Result<String, LexError> {
    let mut src = String::new();
    let mut depth = 0;
    while let Some((_, c)) = chars.next() {
        match c {
            ')' if depth == 0 => return Ok(src),
            ')' => depth -= 1,
            '(' => depth += 1,
            '\'' | '"' => {
                copy_quoted(chars, c, &mut src)?;
                continue;
            }
            '\\' => {
                src.push(c);
                if let Some((_, escaped)) = chars.next() {
                    src.push(escaped);
                }
                continue;
            }
            // A comment can hold a `)`: `$(cmd # why)` + newline + `)`
            '#' if src.is_empty() || src.ends_with(char::is_whitespace) => {
                src.push(c);
                while let Some((_, c)) = chars.next_if(|&(_, c)| c != '\n') {
                    src.push(c);
                }
                continue;
            }
            _ => {}
        }
        src.push(c);
    }
    Err(LexError::Incomplete("unclosed $("))
}

/// A quoted piece inside `$( )`, from its opening `quote` to its closing one,
/// copied into `src` unchanged. Inside `"..."`, `\` and a nested `$( )` are
/// stepped over too.
fn copy_quoted(
    chars: &mut Peekable<CharIndices>,
    quote: char,
    src: &mut String,
) -> Result<(), LexError> {
    src.push(quote);
    while let Some((_, c)) = chars.next() {
        src.push(c);
        if c == quote {
            return Ok(());
        }
        if quote == '"' && c == '\\' {
            src.extend(chars.next().map(|(_, escaped)| escaped));
        } else if quote == '"' && c == '$' && chars.next_if(|&(_, n)| n == '(').is_some() {
            src.push('(');
            src.push_str(&read_command_sub(chars)?);
            src.push(')');
        }
    }
    Err(LexError::Incomplete(if quote == '"' {
        "unclosed \""
    } else {
        "unclosed '"
    }))
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
/// Too big for a `u32` is still an io number (saturated), so it gets refused
/// instead of silently becoming an argument to the command.
fn io_number(w: &Word) -> Option<u32> {
    let digits = w
        .as_unquoted()
        .filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))?;
    Some(digits.parse().unwrap_or(u32::MAX))
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
                TokenKind::Word(w) => Some(w.joined_text()),
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
                TokenKind::Word(w) => K::W(w.joined_text()),
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
    fn dollar_forms_become_params_except_in_single_quotes() {
        let parts = |src| match &lex(src).unwrap()[..] {
            [
                Token {
                    kind: TokenKind::Word(w),
                    ..
                },
            ] => w.parts.clone(),
            other => panic!("{other:?}"),
        };
        let param = |name: &str, braced, quoted| WordPart::Param {
            name: name.into(),
            braced,
            quoted,
        };
        assert_eq!(parts("$x_1"), [param("x_1", false, false)]);
        assert_eq!(
            parts("a${b}c"),
            [lit("a", false), param("b", true, false), lit("c", false)]
        );
        assert_eq!(parts("$?"), [param("?", false, false)]);
        assert_eq!(
            parts(r#""a $x""#),
            [lit("a ", true), param("x", false, true)]
        );
        assert_eq!(parts("'$x'"), [lit("$x", true)]);
        assert_eq!(parts(r"\$x"), [lit("$", true), lit("x", false)]);
        // `$` before anything else is just a character
        assert_eq!(parts("a$"), [lit("a$", false)]);
        assert_eq!(parts("$1"), [lit("$1", false)]);
        assert_eq!(parts(r#""$""#), [lit("$", true)]);
        assert_eq!(lex("${x"), Err(LexError::Incomplete("unclosed ${")));
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
        assert_eq!(
            kinds("99999999999>x"),
            [K::N(u32::MAX), K::O(Great), w("x")]
        );
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
