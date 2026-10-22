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

use std::fmt;

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

impl AndOr {
    /// Every pipeline in the chain, in order, ignoring the `&&` / `||` between them.
    pub fn pipelines(&self) -> impl Iterator<Item = &Pipeline> {
        std::iter::once(&self.first).chain(self.rest.iter().map(|(_, p)| p))
    }
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

impl Command {
    /// The redirects of any kind of command.
    pub fn redirects(&self) -> &[Redirect] {
        match self {
            Command::Simple { redirects, .. }
            | Command::Subshell { redirects, .. }
            | Command::Group { redirects, .. } => redirects,
        }
    }

    /// The commands inside `( )` or `{ }`; `None` for a simple command.
    pub fn body(&self) -> Option<&List> {
        match self {
            Command::Simple { .. } => None,
            Command::Subshell { body, .. } | Command::Group { body, .. } => Some(body),
        }
    }
}

/// `2>&1`, `> out`, `<< EOF`
pub struct Redirect {
    /// The `2` in `2>file`; `None` means the operator's default (0 for `<`, 1 for `>`).
    pub fd: Option<u32>,
    pub op: Op,
    /// The file, the fd to copy (`&1`), or the heredoc's end marker.
    pub target: Word,
}

/// Written back compactly, for messages: `2>&1`, `>out`.
impl fmt::Display for Redirect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(fd) = self.fd {
            write!(f, "{fd}")?;
        }
        write!(f, "{}{}", self.op.as_str(), self.target.joined_text())
    }
}
