//! Where a command's input and output go.
//!
//! Builtins never print directly: they write through `Io`, so redirects and
//! pipes only ever change this file, never the builtins.

use std::io::{self, Write};
use std::process::Stdio;

/// Where one of a command's streams is connected.
/// Later steps add `File`, `Null`, `Memory`, `PipeIn` and `PipeOut`.
pub enum Stream {
    /// Reef's own stdin.
    Stdin,
    /// Reef's own stdout.
    Stdout,
    /// Reef's own stderr.
    Stderr,
}

impl Stream {
    /// A second handle to the same place, for `2>&1`.
    /// Not `Clone`, because duplicating a file or pipe handle can fail.
    pub fn try_clone(&self) -> io::Result<Stream> {
        Ok(match self {
            Stream::Stdin => Stream::Stdin,
            Stream::Stdout => Stream::Stdout,
            Stream::Stderr => Stream::Stderr,
        })
    }

    /// What to give a child process for this stream.
    pub fn into_stdio(self) -> Stdio {
        match self {
            Stream::Stdin => Stdio::inherit(),
            Stream::Stdout => Stdio::from(io::stdout()),
            Stream::Stderr => Stdio::from(io::stderr()),
        }
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Stream::Stdout => io::stdout().lock().write_all(bytes),
            Stream::Stderr => io::stderr().lock().write_all(bytes),
            Stream::Stdin => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "cannot write to stdin",
            )),
        }
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

    /// Handles for a child process: (stdin, stdout, stderr).
    /// Copies the streams, so this `Io` stays usable for the next command.
    pub fn for_child(&self) -> io::Result<(Stdio, Stdio, Stdio)> {
        Ok((
            self.stdin.try_clone()?.into_stdio(),
            self.stdout.try_clone()?.into_stdio(),
            self.stderr.try_clone()?.into_stdio(),
        ))
    }

    /// Call before Reef exits, so buffered output like `echo -n` isn't lost.
    pub fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()?;
        io::stderr().flush()
    }
}
