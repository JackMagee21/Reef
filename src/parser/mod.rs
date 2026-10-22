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
}

impl ParseError {
    /// The message to show. Needs the source to turn the span into a line and column.
    pub fn describe(&self, src: &str) -> String {
        match self {
            ParseError::Incomplete(what) => format!("unexpected end of input: {what}"),
            ParseError::Syntax { message, span } => {
                let before = &src[..span.start];
                let line = before.matches('\n').count() + 1;
                let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
                if line == 1 {
                    format!("{message} (column {column})")
                } else {
                    format!("{message} (line {line}, column {column})")
                }
            }
        }
    }
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
            let background = self.peek_op() == Some(Op::Amp);
            match self.peek_op() {
                Some(Op::Amp | Op::Semi | Op::Newline) => self.pos += 1,
                // The end, `)` or `}`: the top of the loop decides if that's allowed
                _ if self.peek().is_none() || self.peek_closer().is_some() => {}
                _ => return self.unexpected(),
            }
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
            let op = if connector == Connector::And {
                "&&"
            } else {
                "||"
            };
            self.need_command(op)?;
            rest.push((connector, self.pipeline()?));
        }
    }

    fn pipeline(&mut self) -> Result<Pipeline, ParseError> {
        let negated = self.peek_plain_word() == Some("!");
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
        let word = self.peek_plain_word();

        if op == Some(Op::LParen) {
            self.pos += 1;
            let body = self.list(Some(Closer::Paren))?;
            self.pos += 1; // the `)`: list only returns when it's next
            let redirects = self.redirects()?;
            return Ok(Command::Subshell { body, redirects });
        }
        if word == Some("{") {
            self.pos += 1;
            let body = self.list(Some(Closer::Brace))?;
            self.pos += 1; // the `}`
            let redirects = self.redirects()?;
            return Ok(Command::Group { body, redirects });
        }
        if let Some(w) = word.filter(|w| NOT_YET.contains(w)) {
            return Err(syntax(format!("`{w}` is not supported yet"), span));
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
                Ok(Redirect { fd, op, target })
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
    fn peek_plain_word(&self) -> Option<&str> {
        match &self.peek()?.kind {
            TokenKind::Word(w) => w.as_unquoted(),
            _ => None,
        }
    }

    /// `)` or a `}` word, which end a nested list.
    fn peek_closer(&self) -> Option<Closer> {
        if self.peek_op() == Some(Op::RParen) {
            Some(Closer::Paren)
        } else if self.peek_plain_word() == Some("}") {
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
                    if i.background { " &" } else { "" }
                )
            })
            .collect();
        items.join("; ")
    }

    fn and_or(a: &AndOr) -> String {
        let mut s = pipeline(&a.first);
        for (c, p) in &a.rest {
            s += if *c == Connector::And { " && " } else { " || " };
            s += &pipeline(p);
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
                    .map(Word::to_plain)
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
            "unexpected end of input: `{` is missing its `}`"
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
        assert_eq!(err("a ;;"), "unexpected `;;` (column 3)");
        assert_eq!(err("; a"), "unexpected `;` (column 1)");
        assert_eq!(err("a && ; b"), "unexpected `;` (column 6)");
        assert_eq!(err(")"), "unexpected `)` (column 1)");
        assert_eq!(err("( )"), "unexpected `)` (column 3)");
        assert_eq!(err("{ }"), "unexpected `}` (column 3)");
        assert_eq!(err("(a; }"), "unexpected `}` (column 5)");
        assert_eq!(err("(a) b"), "unexpected `b` (column 5)");
        assert_eq!(err("a | }"), "unexpected `}` (column 5)");
        assert_eq!(err("a > ;"), "`>` needs a file name after it (column 3)");
        assert_eq!(err("a 2>"), "`>` needs a file name after it (column 4)");
        assert_eq!(err("while x"), "`while` is not supported yet (column 1)");
        assert_eq!(err("a; fi"), "`fi` is not supported yet (column 4)");
        assert_eq!(err("a\nb )"), "unexpected `)` (line 2, column 3)");
    }
}
