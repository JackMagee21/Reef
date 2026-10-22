//! Where a command's input and output go.
//!
//! Builtins never print directly: they write through `Io`, so redirects and
//! pipes only ever change this file, never the builtins.

use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::process::Stdio;

/// Where one of a command's streams is connected.
/// Later steps add `Memory`, `PipeIn` and `PipeOut`.
pub enum Stream {
    /// Reef's own stdin.
    Stdin,
    /// Reef's own stdout.
    Stdout,
    /// Reef's own stderr.
    Stderr,
    /// An open file, from `> out`, `>> log` or `< in`. `/dev/null` is one of these too.
    File(File),
}

impl Stream {
    /// A second handle to the same place, for `2>&1`.
    /// Not `Clone`, because duplicating a file or pipe handle can fail.
    pub fn try_clone(&self) -> io::Result<Stream> {
        Ok(match self {
            Stream::Stdin => Stream::Stdin,
            Stream::Stdout => Stream::Stdout,
            Stream::Stderr => Stream::Stderr,
            Stream::File(f) => Stream::File(f.try_clone()?),
        })
    }

    /// What to give a child process for this stream.
    pub fn into_stdio(self) -> Stdio {
        match self {
            Stream::Stdin => Stdio::inherit(),
            Stream::Stdout => Stdio::from(io::stdout()),
            Stream::Stderr => Stdio::from(io::stderr()),
            Stream::File(f) => Stdio::from(f),
        }
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Stream::Stdout => io::stdout().lock().write_all(bytes),
            Stream::Stderr => io::stderr().lock().write_all(bytes),
            Stream::File(f) => f.write_all(bytes),
            Stream::Stdin => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "cannot write to stdin",
            )),
        }
    }
}

/// An OS error in the words shells use (`No such file or directory`), rather
/// than Rust's (`The system cannot find the file specified. (os error 2)`).
pub fn describe_error(e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::NotFound => "No such file or directory".into(),
        io::ErrorKind::PermissionDenied => "Permission denied".into(),
        io::ErrorKind::IsADirectory => "Is a directory".into(),
        io::ErrorKind::NotADirectory => "Not a directory".into(),
        _ => e.to_string(),
    }
}

/// The three streams every command gets.
pub struct Io {
    pub stdin: Stream,
    pub stdout: Stream,
    pub stderr: Stream,
}

impl Io {
    /// No redirects or pipes: everything goes to Reef's own console.
    pub fn terminal() -> Io {
        Io {
            stdin: Stream::Stdin,
            stdout: Stream::Stdout,
            stderr: Stream::Stderr,
        }
    }

    /// Normal output, for builtins.
    pub fn write_out(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stdout.write_all(bytes)
    }

    /// Error output, for builtins.
    pub fn write_err(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stderr.write_all(bytes)
    }

    /// Report an error as `reef: <message>`, e.g. `io.error(format_args!("cd: {dir}: Not a directory"))`.
    pub fn error(&mut self, message: impl fmt::Display) {
        // If stderr itself is broken there's nowhere left to report it
        let _ = self.write_err(format!("reef: {message}\n").as_bytes());
    }

    /// A second `Io` pointing at the same places, e.g. to apply a command's redirects to.
    pub fn try_clone(&self) -> io::Result<Io> {
        Ok(Io {
            stdin: self.stdin.try_clone()?,
            stdout: self.stdout.try_clone()?,
            stderr: self.stderr.try_clone()?,
        })
    }

    /// Stream number `fd`: 0 is stdin, 1 stdout, 2 stderr. `None` above 2.
    pub fn stream(&self, fd: u32) -> Option<&Stream> {
        match fd {
            0 => Some(&self.stdin),
            1 => Some(&self.stdout),
            2 => Some(&self.stderr),
            _ => None,
        }
    }

    pub fn stream_mut(&mut self, fd: u32) -> Option<&mut Stream> {
        match fd {
            0 => Some(&mut self.stdin),
            1 => Some(&mut self.stdout),
            2 => Some(&mut self.stderr),
            _ => None,
        }
    }

    /// Handles for a child process: (stdin, stdout, stderr).
    /// Copies the streams, so this `Io` stays usable for the next command.
    pub fn for_child(&self) -> io::Result<(Stdio, Stdio, Stdio)> {
        let copy = self.try_clone()?;
        Ok((
            copy.stdin.into_stdio(),
            copy.stdout.into_stdio(),
            copy.stderr.into_stdio(),
        ))
    }

    /// Call before Reef exits, so buffered output like `echo -n` isn't lost.
    pub fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()?;
        io::stderr().flush()
    }
}
