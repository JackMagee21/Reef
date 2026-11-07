//! Turns tokens into the command structure in `ast.rs`, by recursive descent:
//! one function per grammar rule, each calling the rules below it.
//!
//! The grammar so far (a POSIX subset; `?` optional, `*` repeated):
//!
//! ```text
//! list      := newline* (and_or (sep newline*)?)*         sep is `;`, `&` or a newline
//! and_or    := pipeline ((`&&` | `||`) newline* pipeline)*
//! pipeline  := `!`? command (`|` newline* command)*
//! command   := `(` list `)` redirect* | `{` list `}` redirect* | simple
//! simple    := (word | redirect)+
//! redirect  := io_number? redirect_op word
//! ```

use crate::ast::{AndOr, Command, Connector, Item, List, Pipeline, Redirect};
use crate::lexer::{self, LexError, Op, Span, Token, TokenKind};

#[derive(Debug, PartialEq)]
pub enum ParseError {
    /// The input stopped before the command was finished: `echo a &&`, `{ echo`, `'abc`.
    /// Piped mode and the prompt read another line instead of reporting it.
    Incomplete(String),
    /// Wrong input, pointing at the token where it went wrong.
    Syntax { message: String, span: Span },
    /// Valid shell, but a feature Reef doesn't have yet: `if`, `while`, `case`...
    NotYet { what: String, span: Span },
}

impl ParseError {
    /// What kind of problem it is, to put before the message: `syntax error` or `not supported yet`.
    pub fn label(&self) -> &'static str {
        match self {
            ParseError::Incomplete(_) | ParseError::Syntax { .. } => "syntax error",
            ParseError::NotYet { .. } => "not supported yet",
        }
    }

    /// The whole message: what went wrong, then the line with a `^` under it.
    pub fn describe(&self, src: &str) -> String {
        format!("{}\n{}", self.message(src), self.pointer(src))
    }

    /// What went wrong, in one line. `(line 2)` when it isn't on the first line.
    pub fn message(&self, src: &str) -> String {
        let what = match self {
            ParseError::Incomplete(what) => &format!("unexpected end of input: {what}"),
            ParseError::Syntax { message, .. } => message,
            ParseError::NotYet { what, .. } => what,
        };
        format!("{what}{}", line_note(src, self.span(src)))
    }

    /// The line it went wrong on, with a `^` under the spot.
    pub fn pointer(&self, src: &str) -> String {
        pointer(src, self.span(src))
    }

    /// Where it went wrong. Unfinished input points just past its end, where
    /// the missing part would go.
    fn span(&self, src: &str) -> Span {
        match self {
            ParseError::Incomplete(_) => {
                let end = src.trim_end().len();
                Span { start: end, end }
            }
            ParseError::Syntax { span, .. } | ParseError::NotYet { span, .. } => *span,
        }
    }
}

/// ` (line 2)` when `span` isn't on the first line of `src`, else nothing.
pub fn line_note(src: &str, span: Span) -> String {
    let start = src.floor_char_boundary(span.start);
    match src[..start].matches('\n').count() + 1 {
        1 => String::new(),
        line => format!(" (line {line})"),
    }
}

/// The line of `src` that `span` starts on, indented two spaces, with a `^`
/// under each character of the span (at least one, so the end of input shows):
///
/// ```text
///   echo a )
///          ^
/// ```
///
/// Counts characters, not bytes, so `é` doesn't push the `^` off its target.
// shortcut: wide characters (emoji, CJK) fill two columns, so the `^` drifts after one
pub fn pointer(src: &str, span: Span) -> String {
    // A span should never split a character, but slicing there would crash Reef
    let start = src.floor_char_boundary(span.start);
    let line_start = src[..start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = src[start..].find('\n').map_or(src.len(), |i| start + i);
    let end = src.floor_char_boundary(span.end.clamp(start, line_end));
    // A tab stays a tab, so the terminal moves both lines the same distance
    let indent: String = src[line_start..start]
        .chars()
        .map(|c| if c == '\t' { '\t' } else { ' ' })
        .collect();
    let width = src[start..end].chars().count().max(1);
    let line = src[line_start..line_end].trim_end_matches('\r');
    format!("  {line}\n  {indent}{}", "^".repeat(width))
}

/// Lex and parse a whole input.
pub fn parse(src: &str) -> Result<List, ParseError> {
    let tokens = lexer::lex(src)
        .map_err(|LexError::Incomplete(what)| ParseError::Incomplete(what.into()))?;
    Parser {
        src,
        tokens,
        pos: 0,
    }
    .list(None)
}

/// Reserved words for control flow that comes in a later step. At the start of
/// a command they're an error rather than being run as a program called `if`.
const NOT_YET: &[&str] = &[
    "if", "then", "else", "elif", "fi", "while", "until", "for", "do", "done", "case", "esac",
];

/// What ends a nested list.
#[derive(Clone, Copy, PartialEq)]
enum Closer {
    Paren,
    Brace,
}

struct Parser<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser<'_> {
    fn list(&mut self, closer: Option<Closer>) -> Result<List, ParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_newlines();
            if self.peek().is_none() {
                return match closer {
                    None => Ok(items),
                    Some(Closer::Paren) => Err(incomplete("`(` is missing its `)`")),
                    Some(Closer::Brace) => Err(incomplete("`{` is missing its `}`")),
                };
            }
            if let Some(found) = self.peek_closer() {
                // `( )` and `{ }` need at least one command inside
                if closer == Some(found) && !items.is_empty() {
                    return Ok(items);
                }
                return self.unexpected();
            }

            let and_or = self.and_or()?;
            // What ends the command: `&` (run it in the background), `;` or a newline
            let background = match self.peek_op() {
                Some(Op::Amp) => {
                    self.pos += 1;
                    Some(self.tokens[self.pos - 1].span)
                }
                Some(Op::Semi | Op::Newline) => {
                    self.pos += 1;
                    None
                }
                // The end, `)` or `}`: the top of the loop decides if that's allowed
                _ if self.peek().is_none() || self.peek_closer().is_some() => None,
                _ => return self.unexpected(),
            };
            items.push(Item { and_or, background });
        }
    }

    fn and_or(&mut self) -> Result<AndOr, ParseError> {
        let first = self.pipeline()?;
        let mut rest = Vec::new();
        loop {
            let connector = match self.peek_op() {
                Some(Op::AndIf) => Connector::And,
                Some(Op::OrIf) => Connector::Or,
                _ => return Ok(AndOr { first, rest }),
            };
            self.pos += 1;
            // `a &&` can continue on the next line
            self.skip_newlines();
            self.need_command(connector.as_str())?;
            rest.push((connector, self.pipeline()?));
        }
    }

    fn pipeline(&mut self) -> Result<Pipeline, ParseError> {
        let negated = self.peek_unquoted_word() == Some("!");
        if negated {
            self.pos += 1;
            self.need_command("!")?;
        }
        let mut commands = vec![self.command()?];
        while self.peek_op() == Some(Op::Pipe) {
            self.pos += 1;
            self.skip_newlines();
            self.need_command("|")?;
            commands.push(self.command()?);
        }
        Ok(Pipeline { negated, commands })
    }

    fn command(&mut self) -> Result<Command, ParseError> {
        let Some(token) = self.peek() else {
            return Err(incomplete("a command is missing"));
        };
        let span = token.span;
        let op = match token.kind {
            TokenKind::Op(op) => Some(op),
            _ => None,
        };
        let word = self.peek_unquoted_word();

        if op == Some(Op::LParen) {
            self.pos += 1;
            let body = self.list(Some(Closer::Paren))?;
            self.close(Closer::Paren)?;
            let redirects = self.redirects()?;
            return Ok(Command::Subshell { body, redirects });
        }
        if word == Some("{") {
            self.pos += 1;
            let body = self.list(Some(Closer::Brace))?;
            self.close(Closer::Brace)?;
            let redirects = self.redirects()?;
            return Ok(Command::Group { body, redirects });
        }
        if let Some(w) = word.filter(|w| NOT_YET.contains(w)) {
            return Err(ParseError::NotYet {
                what: format!("`{w}`"),
                span,
            });
        }
        if word == Some("}") || op.is_some_and(|op| !op.is_redirect()) {
            return self.unexpected();
        }
        self.simple()
    }

    /// Words and redirects, in any order: `> out echo hi` is `echo hi > out`.
    fn simple(&mut self) -> Result<Command, ParseError> {
        let mut words = Vec::new();
        let mut redirects = Vec::new();
        loop {
            if let Some(TokenKind::Word(w)) = self.peek().map(|t| &t.kind) {
                words.push(w.clone());
                self.pos += 1;
            } else if self.peek_is_redirect() {
                redirects.push(self.redirect()?);
            } else {
                return Ok(Command::Simple { words, redirects });
            }
        }
    }

    /// Redirects after `( )` or `{ }`.
    fn redirects(&mut self) -> Result<Vec<Redirect>, ParseError> {
        let mut redirects = Vec::new();
        while self.peek_is_redirect() {
            redirects.push(self.redirect()?);
        }
        Ok(redirects)
    }

    fn redirect(&mut self) -> Result<Redirect, ParseError> {
        let start = self.peek().map_or(0, |t| t.span.start);
        let fd = match self.peek().map(|t| &t.kind) {
            Some(&TokenKind::IoNumber(n)) => {
                self.pos += 1;
                Some(n)
            }
            _ => None,
        };
        // The lexer only makes an io number when a redirect operator follows it
        let Some(Token {
            kind: TokenKind::Op(op),
            span,
        }) = self.peek().cloned()
        else {
            return Err(incomplete("a redirect is missing its operator"));
        };
        self.pos += 1;
        match self.peek().map(|t| &t.kind) {
            Some(TokenKind::Word(target)) => {
                let target = target.clone();
                self.pos += 1;
                let span = Span {
                    start,
                    end: target.span.end,
                };
                Ok(Redirect {
                    fd,
                    op,
                    target,
                    span,
                })
            }
            _ => Err(syntax(
                format!("`{}` needs a file name after it", op.as_str()),
                span,
            )),
        }
    }

    // ---- looking at the next token ----

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_op(&self) -> Option<Op> {
        match self.peek()?.kind {
            TokenKind::Op(op) => Some(op),
            _ => None,
        }
    }

    fn peek_is_redirect(&self) -> bool {
        match self.peek().map(|t| &t.kind) {
            Some(TokenKind::IoNumber(_)) => true,
            Some(TokenKind::Op(op)) => op.is_redirect(),
            _ => false,
        }
    }

    /// The next token's text if it's an unquoted plain word. Reserved words like
    /// `{` and `!` only count when written exactly like that: `'{'` is just text.
    fn peek_unquoted_word(&self) -> Option<&str> {
        match &self.peek()?.kind {
            TokenKind::Word(w) => w.as_unquoted(),
            _ => None,
        }
    }

    /// `)` or a `}` word, which end a nested list.
    fn peek_closer(&self) -> Option<Closer> {
        if self.peek_op() == Some(Op::RParen) {
            Some(Closer::Paren)
        } else if self.peek_unquoted_word() == Some("}") {
            Some(Closer::Brace)
        } else {
            None
        }
    }

    fn skip_newlines(&mut self) {
        while self.peek_op() == Some(Op::Newline) {
            self.pos += 1;
        }
    }

    /// Step past the `)` or `}` that ends a group, checking it's really there.
    fn close(&mut self, closer: Closer) -> Result<(), ParseError> {
        if self.peek_closer() == Some(closer) {
            self.pos += 1;
            Ok(())
        } else {
            self.unexpected()
        }
    }

    /// After `&&`, `||`, `|` or `!` there must be a command; at the end, more input may bring it.
    fn need_command(&self, after: &str) -> Result<(), ParseError> {
        match self.peek() {
            Some(_) => Ok(()),
            None => Err(incomplete(&format!("`{after}` needs a command after it"))),
        }
    }

    fn unexpected<T>(&self) -> Result<T, ParseError> {
        let Some(token) = self.peek() else {
            return Err(incomplete("the command isn't finished"));
        };
        let text = match token.kind {
            TokenKind::Op(op) => op.as_str(),
            _ => &self.src[token.span.start..token.span.end],
        };
        Err(syntax(format!("unexpected `{text}`"), token.span))
    }
}

fn incomplete(what: &str) -> ParseError {
    ParseError::Incomplete(what.to_string())
}

fn syntax(message: String, span: Span) -> ParseError {
    ParseError::Syntax { message, span }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::word::Word;

    /// The tree as one line: `[a]` is a simple command, `{...}` a group, `(...)` a subshell.
    fn show(src: &str) -> String {
        list(&parse(src).unwrap())
    }

    fn err(src: &str) -> String {
        match parse(src) {
            Ok(_) => panic!("{src:?} parsed"),
            Err(e) => e.describe(src),
        }
    }

    fn list(l: &List) -> String {
        let items: Vec<String> = l
            .iter()
            .map(|i| {
                format!(
                    "{}{}",
                    and_or(&i.and_or),
                    if i.background.is_some() { " &" } else { "" }
                )
            })
            .collect();
        items.join("; ")
    }

    fn and_or(a: &AndOr) -> String {
        let mut s = pipeline(&a.first);
        for (c, p) in &a.rest {
            s += &format!(" {} {}", c.as_str(), pipeline(p));
        }
        s
    }

    fn pipeline(p: &Pipeline) -> String {
        let commands: Vec<String> = p.commands.iter().map(command).collect();
        format!(
            "{}{}",
            if p.negated { "! " } else { "" },
            commands.join(" | ")
        )
    }

    fn command(c: &Command) -> String {
        let after: String = c.redirects().iter().map(|r| format!(" {r}")).collect();
        match c {
            Command::Simple { words, .. } => {
                let parts: Vec<String> = words
                    .iter()
                    .map(Word::joined_text)
                    .chain(c.redirects().iter().map(Redirect::to_string))
                    .collect();
                format!("[{}]", parts.join(" "))
            }
            Command::Subshell { body, .. } => format!("({}){after}", list(body)),
            Command::Group { body, .. } => format!("{{{}}}{after}", list(body)),
        }
    }

    #[test]
    fn lists_split_on_semicolons_newlines_and_ampersands() {
        assert_eq!(show("a; b\nc &"), "[a]; [b]; [c] &");
        assert_eq!(show("a & b"), "[a] &; [b]");
        assert_eq!(show("a;"), "[a]");
        assert_eq!(show("\n\n# only a comment\n"), "");
        assert_eq!(show(""), "");
    }

    #[test]
    fn and_or_chains_left_to_right() {
        assert_eq!(show("a && b || c"), "[a] && [b] || [c]");
        assert_eq!(show("a &&\n\n b"), "[a] && [b]");
    }

    #[test]
    fn pipelines_and_negation() {
        assert_eq!(show("! a | b |\n c"), "! [a] | [b] | [c]");
        assert_eq!(show("! a && b"), "! [a] && [b]");
    }

    #[test]
    fn redirects_can_go_anywhere_in_a_simple_command() {
        assert_eq!(show("echo hi >out 2>&1 <in"), "[echo hi >out 2>&1 <in]");
        assert_eq!(show("> f echo hi"), "[echo hi >f]");
        assert_eq!(show(">f"), "[>f]");
    }

    #[test]
    fn groups_and_subshells_nest() {
        assert_eq!(
            show("{ a; b; } && (c\nd) >f"),
            "{[a]; [b]} && ([c]; [d]) >f"
        );
        assert_eq!(show("( (a) )"), "(([a]))");
        assert_eq!(show("{\na\n}"), "{[a]}");
    }

    #[test]
    fn reserved_words_only_count_at_the_start_and_unquoted() {
        assert_eq!(show("echo { } ! if"), "[echo { } ! if]");
        assert_eq!(show("'{' a"), "[{ a]");
        assert_eq!(show("'if' x"), "[if x]");
        // `}` without `;` before it is just an argument, so the group isn't closed
        assert_eq!(
            err("{ echo a }"),
            "unexpected end of input: `{` is missing its `}`\n  { echo a }\n            ^"
        );
    }

    #[test]
    fn unfinished_input_is_incomplete() {
        for (src, what) in [
            ("a &&", "`&&` needs a command after it"),
            ("a ||\n", "`||` needs a command after it"),
            ("a |", "`|` needs a command after it"),
            ("!", "`!` needs a command after it"),
            ("{ a;", "`{` is missing its `}`"),
            ("(a", "`(` is missing its `)`"),
            ("echo 'x", "unclosed '"),
        ] {
            assert_eq!(parse(src).err(), Some(incomplete(what)), "{src:?}");
        }
    }

    #[test]
    fn syntax_errors_point_at_the_token() {
        // (input, message, the `^` line without its two-space indent)
        for (src, message, caret) in [
            ("a ;;", "unexpected `;;`", "  ^^"),
            ("; a", "unexpected `;`", "^"),
            ("a && ; b", "unexpected `;`", "     ^"),
            (")", "unexpected `)`", "^"),
            ("( )", "unexpected `)`", "  ^"),
            ("{ }", "unexpected `}`", "  ^"),
            ("(a; }", "unexpected `}`", "    ^"),
            ("(a) b", "unexpected `b`", "    ^"),
            ("a | }", "unexpected `}`", "    ^"),
            ("a > ;", "`>` needs a file name after it", "  ^"),
            ("a 2>", "`>` needs a file name after it", "   ^"),
            ("while x", "`while`", "^^^^^"),
            ("a; fi", "`fi`", "   ^^"),
        ] {
            assert_eq!(err(src), format!("{message}\n  {src}\n  {caret}"), "{src:?}");
        }
        let label = |src| parse(src).err().unwrap().label();
        assert_eq!(label("while x"), "not supported yet");
        assert_eq!(label("a ;;"), "syntax error");
        assert_eq!(label("a &&"), "syntax error");
    }

    #[test]
    fn errors_after_the_first_line_say_which_line() {
        assert_eq!(err("a\nb )"), "unexpected `)` (line 2)\n  b )\n    ^");
        assert_eq!(
            err("a &&\n"),
            "unexpected end of input: `&&` needs a command after it\n  a &&\n      ^"
        );
    }

    #[test]
    fn words_redirects_and_ampersands_remember_where_they_were_typed() {
        let src = "echo é ${x} 2>f >  out &";
        let list = parse(src).unwrap();
        let text = |span: Span| &src[span.start..span.end];
        assert_eq!(list[0].background.map(text), Some("&"));
        let Command::Simple { words, redirects } = &list[0].and_or.first.commands[0] else {
            panic!("not a simple command");
        };
        let words: Vec<&str> = words.iter().map(|w| text(w.span)).collect();
        assert_eq!(words, ["echo", "é", "${x}"]);
        let redirects: Vec<&str> = redirects.iter().map(|r| text(r.span)).collect();
        assert_eq!(redirects, ["2>f", ">  out"]);
    }

    fn at(src: &str, start: usize, end: usize) -> String {
        pointer(src, Span { start, end })
    }

    #[test]
    fn the_pointer_counts_characters_not_bytes() {
        // `é` is two bytes: the `)` is byte 8 but character 7
        assert_eq!(at("echo é )", 8, 9), "  echo é )\n         ^");
        // tabs stay tabs, so both lines line up however wide the terminal draws them
        assert_eq!(at("\techo )", 6, 7), "  \techo )\n  \t     ^");
        // a whole word gets one `^` per character
        assert_eq!(at("echo ${x:-1}", 5, 12), "  echo ${x:-1}\n       ^^^^^^^");
    }

    #[test]
    fn the_pointer_shows_only_its_own_line() {
        assert_eq!(at("echo a\necho b )\necho c", 14, 15), "  echo b )\n         ^");
        assert_eq!(at("echo a )\r\n", 7, 8), "  echo a )\n         ^");
        // a span running past the end of its line stops there
        assert_eq!(at("ab\ncd", 0, 5), "  ab\n  ^^");
    }

    #[test]
    fn the_pointer_never_crashes_on_a_bad_span() {
        // past the end: one `^` after the last character
        assert_eq!(at("a &&", 4, 4), "  a &&\n      ^");
        assert_eq!(at("a", 99, 99), "  a\n   ^");
        // inside `é` (bytes 0..2): moved back to its start
        assert_eq!(at("é", 1, 2), "  é\n  ^");
        assert_eq!(at("", 0, 0), "  \n  ^");
    }
}
