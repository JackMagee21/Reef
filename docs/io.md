# Io and Stream

`src/io.rs` decides where a command's input and output go. Every command, builtin or external, gets an `Io` with three streams: `stdin`, `stdout` and `stderr`.

## Why builtins never print directly

Builtins write with `io.write_out(...)` and `io.write_err(...)`, never `println!` or `eprintln!`.

That way, when redirects and pipes arrive (**planned**), they only swap which stream `Io` holds. `echo hi > file` will work without `echo` knowing files exist. RShell printed directly and then needed a special case just for `echo > file`, while redirects on every other builtin were silently ignored.

## Stream

A `Stream` is one of a fixed set of places, not "anything writable". The reason: an external program can only be given a real OS handle (the console, a file, a pipe), never a Rust object.

| Kind | Meaning | Status |
|---|---|---|
| `Stdin`, `Stdout`, `Stderr` | Reef's own three console streams | built |
| `File` | an open file, from `> out.txt`, `>> log`, `< in.txt` | planned |
| `Null` | discard everything (`/dev/null`, `NUL`) | planned |
| `Memory` | bytes in memory, for builtin tests and `$(...)` | planned |
| `PipeIn` / `PipeOut` | the two ends of a pipe between commands | planned |

Reef's own stdout and stderr are separate kinds, not one "terminal" kind. With `2>&1`, stderr becomes a copy of stdout. If both were just "the terminal", the copy couldn't know it should follow stdout when Reef's own output is redirected.

## What each method is for

| Method | Used by | Does |
|---|---|---|
| `Io::terminal()` | `main.rs` | an `Io` where everything goes to Reef's console |
| `write_out` / `write_err` | builtins | write bytes; returns an error instead of panicking |
| `for_child()` | `run()` | gives an external program copies of the three handles, so the `Io` is reusable afterwards |
| `flush()` | `main.rs`, `repl.rs`, `run()` | push buffered output to the console |
| `Stream::try_clone` | `for_child`, later `2>&1` | a second handle to the same place. Not `Clone`, because duplicating a file or pipe handle can fail |

## Rules

- **Flush before exiting.** `std::process::exit` skips cleanup, so `main` flushes first, or `echo -n` output can be lost.
- **Flush before starting a program.** Anything a builtin wrote must appear before the program's own output.
- **Write errors are returned, not ignored.** A builtin whose output fails returns status 1. The only `let _ =` allowed is when writing to stderr itself fails, because there's nowhere left to report it.

## Adding a new stream kind (planned work)

Add the variant, then one arm in each of `try_clone`, `into_stdio` and `write_all`. The compiler lists every `match` that needs it. Nothing outside `io.rs` changes.
