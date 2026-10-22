# Redirects

`src/exec/redirect.rs` turns a command's redirects into the `Io` it runs with. Builtins and programs never know they were redirected: `echo` writes to "its stdout", and that happens to be a file.

## The operators

| Write | Means | Opens the file |
|---|---|---|
| `> f` | stdout to `f` | create, or empty it first |
| `>> f` | stdout to the end of `f` | create, or append |
| `< f` | stdin from `f` | read; it must exist |
| `<> f` | stdin from and to `f` | read and write, create, don't empty |
| `>\| f` | same as `>` (only differs once `noclobber` exists) | |
| `2> f` | a numbered stream: 0 stdin, 1 stdout, 2 stderr | |
| `2>&1` | stream 2 becomes a copy of stream 1 **as it is right now** | nothing |
| `<&0` | the same for input | nothing |

Without a number, input operators (`<`, `<&`, `<>`) apply to 0 and output operators to 1.

Redirects can go anywhere in a command (`> out echo hi`), and on groups and subshells, where they apply to everything inside: `{ echo a; echo b; } > both`.

## Order matters

Redirects apply **left to right**, and `2>&1` copies where stdout points *at that moment*:

| Write | stdout | stderr |
|---|---|---|
| `cmd > out 2>&1` | `out` | `out` (copied after stdout moved) |
| `cmd 2>&1 > out` | `out` | the screen (copied before stdout moved) |

That's also why Reef's own stdout and stderr are separate `Stream` kinds (see [streams.md](streams.md)).

## Paths

- Relative to the shell's directory (`sh.cwd()`), so `cd sub && echo hi > f` writes `sub/f`.
- `/dev/null` works on Windows too: the platform layer maps it (and `NUL`) to Windows' null device, so scripts written for Linux run unchanged.

## When a redirect fails

The command **doesn't run**, the status is 1, and a script carries on with the next command, like bash:

```
reef: missing: No such file or directory
reef: 9: Bad file descriptor
```

Messages use the usual shell wording (`No such file or directory`, `Permission denied`), not Rust's. `io::describe_error` does that, and `cd` uses it too.

## Not supported yet

Rejected before anything runs, like other unfinished features:

| Write | Why it waits |
|---|---|
| `<<EOF`, `<<-EOF` | heredocs: the text comes from the lines after the command |
| `3> f` | streams above 2 need `Io` to hold more than three |
| `>&-` | closing a stream |

Unlike bash, `>&word` where `word` isn't a number is an error (`word: Bad file descriptor`), not "send stdout and stderr to the file `word`". POSIX leaves it undefined, and an error is safer than quietly creating a file.
