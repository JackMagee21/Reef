# Builtins

A builtin is a command Reef runs itself, as a Rust function, instead of starting a separate program.

## Why some commands must be builtins

- **They change the shell.** `cd`, `export` and `exit` change Reef's own state. A separate program only changes its own copy, which vanishes when it exits.
- **They must work everywhere.** Plain Windows has no `echo.exe`, `true.exe` or `false.exe` (Git for Windows adds some, but Reef can't rely on it).
- **Speed.** Starting a process costs milliseconds on Windows, and scripts call `echo` and `test` constantly.

## Today

| Command | File | Behavior |
|---|---|---|
| `echo [-n] [args...]` | `builtins/echo.rs` | arguments joined by single spaces, then a newline unless the **first** argument is exactly `-n`. No escape sequences (`\n`) and no other options, on purpose: shells disagree about them, and scripts use `printf` (planned) instead |
| `true` / `false` | `builtins/true_cmd.rs`, `false_cmd.rs` | ignore their arguments, return 0 / 1 |
| `exit [n]` | `builtins/exit.rs` | exits with `n` mod 256 (`exit 300` → 44, `exit -1` → 255), or with `$?` when no `n` is given. `exit abc` prints an error and exits with 2. `exit 1 2` prints "too many arguments", returns 1 and does **not** exit |
| `pwd` | `builtins/pwd.rs` | prints `sh.cwd()`. `-L`/`-P` are ignored |
| `cd [dir \| -]` | `builtins/cd.rs` | no argument: `$HOME` (on Windows, `$USERPROFILE` if `HOME` is unset). `-`: `$OLDPWD`, and prints it. Paths are relative to `sh.cwd()`. **Physical**: `..` is the real parent, even through a symlink. A directory without search (execute) permission is refused. On success sets `PWD` and `OLDPWD` (exported). On failure prints why, returns 1 and stays put |
| `type [name...]` | `builtins/type_cmd.rs` | for each name, prints `name is a shell builtin` or `name is <path>`, checking builtins first, then `PATH` (with `PATHEXT` on Windows). A missing name prints `reef: type: name: not found`; the rest are still checked, and the status is 1 if any was missing. No options (`-t`, `-a`, `-p`) yet |
| `export [name[=value]...]` | `builtins/export.rs` | `name=value` sets and exports; `name` exports it if it's set. With no arguments or `-p`, lists exported variables sorted, as `export name='value'` (re-runnable). Names must be a letter or `_`, then letters, digits or `_`: a bad one prints `reef: export: `1x': not a valid identifier`, the rest are still exported, and the status is 1. `export x=$y` doesn't split `$y` |
| `unset [-v] [name...]` | `builtins/unset.rs` | removes each variable, exported or not, so later programs don't get it either. A name that was never set isn't an error. Names are case-insensitive on Windows (`unset path` removes `PATH`). An invalid name is reported like `export` does and makes the status 1. `-v` is accepted; no `-f` yet, since there are no functions |

`echo hi -n` prints `hi -n`, because `-n` only counts as the first argument.

### How `exit` stops the shell

`exit` never calls `std::process::exit`. That would skip the final flush, and later it would kill the whole shell from inside a subshell. Instead it sets `sh.stop_requested`, and whatever is running commands (the prompt, piped mode, the executor) stops after the current command and exits with its status.

## The shape every builtin has

Every builtin takes the same three things and returns an exit status:

- `&mut Shell`: the shell's state (directory, variables, `$?`)
- `&[String]`: the arguments, where the first one is the command name itself
- `&mut Io`: where to read and write

Unused parameters get a leading `_` (for example `_sh`). Keeping one shape means every builtin fits in the same table.

## The table

`builtins/mod.rs` has `BUILTINS`, the **only** list of builtins in Reef, and `lookup(name)`, which the executor calls before searching `PATH`. `type` reads it too, and later completion, `which` and "did you mean" will. RShell had three separate lists that disagreed with each other.

## Adding a builtin

1. **Spec cases first.** Add rows to `CASES` in `tests/spec.rs`: what it should print and what status it returns. Run `cargo test` and watch them fail.
2. **One file per command**, named after it: `builtins/pwd.rs`. Commands with two names share one file (`test` and `[` will live in `test.rs`). Rust keywords can't be file names, so `true` and `false` will need names like `true_cmd.rs`.
3. **Write the function** with the shape above. Two helpers do the common parts:
   - `super::print(io, "name", text)` writes the output and returns the status: 0, or 1 after reporting `reef: name: write error: ...`.
   - `io.error(format_args!("name: {what}"))` reports an error as `reef: name: ...`.

   Errors are a message plus a non-zero status, never a panic.
4. **Declare the module and add one line to `BUILTINS`** in `builtins/mod.rs`.
5. **`make check`** before committing.

## Refused for now

`alias`, `unalias`, `read`, `jobs`, `fg`, `bg`, `umask`, `ulimit`, `hash`, `fc` and `getopts` only work inside a shell. Fedora ships each as a tiny program (`/usr/bin/alias` runs `builtin alias "$@"` in a throwaway `sh`), which would run, change nothing and report success. They sit in `BUILTINS` pointing at `not_yet`, which says `not supported yet` with status 2. Each one gets its own entry when it's built for real.

## Planned, roughly in order

`alias`, `source`, `test`/`[`, then `read`, `printf`, `shift`, `return`, `set`, then `jobs`, `wait`, `history`.

Coreutils like `ls`, `grep` and `cp` are deliberately **not** planned as builtins for now. Real tools already cover them, and they'd pull focus from the shell itself.
