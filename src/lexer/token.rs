//! What the lexer produces: words and operators, each with where it came from.

use crate::word::Word;

/// Where a token sits in the input, as byte offsets: `src[start..end]` is its text.
/// For error messages that point at the problem, and later for highlighting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Word(Word),
    Op(Op),
    /// The digits in `2>file` or `2>&1`: unquoted digits directly before `<` or `>`.
    /// `2 >file` is the word `2` instead.
    IoNumber(u32),
}

/// Every POSIX operator. Reserved words (`if`, `{`, `!`) are not here: whether
/// they're special depends on where they appear, so the parser decides.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    /// `\n`: ends a command, like `;`
    Newline,
    /// `;`
    Semi,
    /// `;;` (ends a `case` branch)
    DSemi,
    /// `&`
    Amp,
    /// `&&`
    AndIf,
    /// `|`
    Pipe,
    /// `||`
    OrIf,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `<`
    Less,
    /// `>`
    Great,
    /// `>>`
    DGreat,
    /// `<&`
    LessAnd,
    /// `>&`
    GreatAnd,
    /// `<>`
    LessGreat,
    /// `>|`
    Clobber,
    /// `<<` (heredoc)
    DLess,
    /// `<<-` (heredoc, leading tabs stripped)
    DLessDash,
}

impl Op {
    /// Operators that a number like the `2` in `2>file` can come before.
    pub fn is_redirect(self) -> bool {
        matches!(
            self,
            Op::Less
                | Op::Great
                | Op::DGreat
                | Op::LessAnd
                | Op::GreatAnd
                | Op::LessGreat
                | Op::Clobber
                | Op::DLess
                | Op::DLessDash
        )
    }
}
