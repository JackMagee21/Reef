# Io and Stream

`src/streams.rs` decides where a command's input and output go. Every command, builtin or external, gets an `Io` with three streams: `stdin`, `stdout` and `stderr`.

## Why builtins never print directly

Builtins write with `io.write_out(...)` and `io.write_err(...)`, never `println!` or `eprintln!`.

That way, redirects only swap which stream `Io` holds: `echo hi > file` works without `echo` knowing files exist (see [redirects.md](redirects.md)). Pipes (**planned**) will work the same way. RShell printed directly and then needed a special case just for `echo > file`, while redirects on every other builtin were silently ignored.

## Stream

A `Stream` is one of a fixed set of places, not "anything writable". The reason: an external program can only be given a real OS handle (the console, a file, a pipe), never a Rust object.

| Kind | Meaning | Status |
|---|---|---|
| `Stdin`, `Stdout`, `Stderr` | Reef's own three console streams | built |
| `File` | an open file, from `> out.txt`, `>> log`, `< in.txt`. `/dev/null` is a `File` too: the OS's null device | built |
| `Memory` | bytes in memory, for builtin tests and `$(...)` | planned |
| `PipeIn` / `PipeOut` | the two ends of a pipe between commands | planned |

Reef's own stdout and stderr are separate kinds, not one "terminal" kind. With `2>&1`, stderr becomes a copy of stdout. If both were just "the terminal", the copy couldn't know it should follow stdout when Reef's own output is redirected.

## What each method is for

| Method | Used by | Does |
|---|---|---|
| `Io::terminal()` | `main.rs` | an `Io` where everything goes to Reef's console |
| `write_out` / `write_err` | builtins | write bytes; returns an error instead of panicking |
| `error(message)` | builtins, `exec`, `main.rs` |
| `describe_error(e)` (a function) | redirects, `cd` | an OS error in shell words: `No such file or directory`, not Rust's `(os error 2)` | report `reef: <message>` on stderr. The one place that ignores a failed write to stderr, since there's nowhere left to report it |
| `try_clone()` | redirects | a second `Io` pointing at the same places, to apply a command's redirects to |
| `stream(fd)` / `stream_mut(fd)` | redirects | stream 0, 1 or 2 by number, for `2>&1` and `2> f` |
| `for_child()` | `exec` | gives an external program copies of the three handles, so the `Io` is reusable afterwards |
| `flush()` | `main.rs`, `repl.rs`, `exec` | push buffered output to the console |
| `Stream::try_clone` | `Io::try_clone`, `2>&1` | a second handle to the same place. Not `Clone`, because duplicating a file or pipe handle can fail |

## Rules

- **Flush before exiting.** `std::process::exit` skips cleanup, so `main` flushes first, or `echo -n` output can be lost.
- **Flush before starting a program, and before applying redirects.** Anything already written for the screen must appear before what comes next.
- **Write errors are returned, not ignored.** A builtin whose output fails returns status 1. The only `let _ =` allowed is when writing to stderr itself fails, because there's nowhere left to report it.

## Adding a new stream kind (planned work)

Add the variant, then one arm in each of `try_clone`, `into_stdio` and `write_all`. The compiler lists every `match` that needs it. Nothing outside `streams.rs` changes.
