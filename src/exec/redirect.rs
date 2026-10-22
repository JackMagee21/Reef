//! Applying `>`, `>>`, `<`, `<>`, `2>&1` and friends: building the `Io` one command runs with.

use std::fs::OpenOptions;

use crate::ast::Redirect;
use crate::lexer::Op;
use crate::platform;
use crate::shell::Shell;
use crate::streams::{Io, Stream, describe_error};

/// A copy of `io` with `redirects` applied left to right. Order matters:
/// `> out 2>&1` sends both streams to `out`, but `2>&1 > out` points stderr at
/// where stdout was *before* it moved, so errors stay on the screen.
///
/// On failure the message (`missing: No such file or directory`) is returned and
/// the command must not run.
pub fn apply(redirects: &[Redirect], sh: &Shell, io: &Io) -> Result<Io, String> {
    let mut io = io.try_clone().map_err(|e| describe_error(&e))?;
    for r in redirects {
        let target = r.target.joined_text();
        let stream = match r.op {
            // `2>&1`: a copy of another stream, as it is right now
            Op::GreatAnd | Op::LessAnd => {
                let bad = || format!("{target}: Bad file descriptor");
                let from: u32 = target.parse().map_err(|_| bad())?;
                let source = io.stream(from).ok_or_else(bad)?;
                source.try_clone().map_err(|e| describe_error(&e))?
            }
            _ => {
                let mut options = OpenOptions::new();
                match r.op {
                    Op::Less => options.read(true),
                    Op::Great | Op::Clobber => options.write(true).create(true).truncate(true),
                    Op::DGreat => options.append(true).create(true),
                    Op::LessGreat => options.read(true).write(true).create(true),
                    // `unsupported` rejects heredocs before anything runs
                    _ => return Err(format!("`{r}` is not supported yet")),
                };
                let path = platform::redirect_path(sh.cwd(), &target);
                let file = options
                    .open(path)
                    .map_err(|e| format!("{target}: {}", describe_error(&e)))?;
                Stream::File(file)
            }
        };
        let fd = r.fd.unwrap_or(default_fd(r.op));
        // `unsupported` rejects fds above 2 before anything runs
        let slot = io
            .stream_mut(fd)
            .ok_or_else(|| format!("{fd}: Bad file descriptor"))?;
        *slot = stream;
    }
    Ok(io)
}

/// The stream a redirect without a number applies to: 0 for input, 1 for output.
fn default_fd(op: Op) -> u32 {
    match op {
        Op::Less | Op::LessAnd | Op::LessGreat | Op::DLess | Op::DLessDash => 0,
        _ => 1,
    }
}
