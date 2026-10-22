//! What the parser builds: the structure of a command line.
//!
//! `echo a && echo b; (cd x)` becomes
//!
//! ```text
//! List
//! ├── Item: AndOr  [echo a]  &&  [echo b]
//! └── Item: AndOr  Subshell( List: [cd x] )
//! ```
//!
//! Later steps add `If`, `While`, `For`, `Case` and functions as new `Command` variants.

use crate::lexer::Op;
use crate::word::Word;

/// Commands separated by `;`, `&` or newlines, run one after another.
pub type List = Vec<Item>;

pub struct Item {
    pub and_or: AndOr,
    /// Ended with `&`: run in the background.
    pub background: bool,
}

/// `a && b || c`: pipelines joined by `&&` / `||`, evaluated left to right.
pub struct AndOr {
    pub first: Pipeline,
    pub rest: Vec<(Connector, Pipeline)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Connector {
    /// `&&`: run the next pipeline only if the last one succeeded
    And,
    /// `||`: run the next pipeline only if the last one failed
    Or,
}

/// `! a | b | c`
pub struct Pipeline {
    /// Starts with `!`: the status is flipped (0 becomes 1, anything else 0).
    pub negated: bool,
    /// Always at least one.
    pub commands: Vec<Command>,
}

pub enum Command {
    /// `echo hi > out`
    Simple {
        words: Vec<Word>,
        redirects: Vec<Redirect>,
    },
    /// `( list )`: runs in a copy of the shell, so `cd` and `exit` inside don't leak out.
    Subshell {
        body: List,
        redirects: Vec<Redirect>,
    },
    /// `{ list; }`: runs in this shell.
    Group {
        body: List,
        redirects: Vec<Redirect>,
    },
}

/// `2>&1`, `> out`, `<< EOF`
pub struct Redirect {
    /// The `2` in `2>file`; `None` means the operator's default (0 for `<`, 1 for `>`).
    pub fd: Option<u32>,
    pub op: Op,
    /// The file, the fd to copy (`&1`), or the heredoc's end marker.
    pub target: Word,
}
