# Builtins

A builtin is a command Reef runs itself, as a Rust function, instead of starting a separate program.

## Why some commands must be builtins

- **They change the shell.** `cd`, `export` and `exit` change Reef's own state. A separate program only changes its own copy, which vanishes when it exits.
- **They must work everywhere.** Windows has no `echo.exe`, `true.exe` or `false.exe`.
- **Speed.** Starting a process costs milliseconds on Windows, and scripts call `echo` and `test` constantly.

## Today

| Command | File | Behavior |
|---|---|---|
| `echo [-n] [args...]` | `builtins/echo.rs` | arguments joined by single spaces, then a newline unless the **first** argument is exactly `-n`. No escape sequences (`\n`) and no other options, on purpose: shells disagree about them, and scripts use `printf` (planned) instead |

`echo hi -n` prints `hi -n`, because `-n` only counts as the first argument.

## The shape every builtin has

Every builtin takes the same three things and returns an exit status:

- `&mut Shell`: the shell's state (directory, variables, `$?`)
- `&[String]`: the arguments, where the first one is the command name itself
- `&mut Io`: where to read and write

Unused parameters get a leading `_` (for example `_sh`). Keeping one shape means every builtin fits in the same table.

## The table

`builtins/mod.rs` has `BUILTINS`, the **only** list of builtins in Reef, and `lookup(name)`, which `run()` calls before searching `PATH`. Later, completion, `type`, `which` and "did you mean" will read this same list. RShell had three separate lists that disagreed with each other.

## Adding a builtin

1. **Spec cases first.** Add rows to `CASES` in `tests/spec.rs`: what it should print and what status it returns. Run `cargo test` and watch them fail.
2. **One file per command**, named after it: `builtins/pwd.rs`. Commands with two names share one file (`test` and `[` will live in `test.rs`). Rust keywords can't be file names, so `true` and `false` will need names like `true_cmd.rs`.
3. **Write the function** with the shape above. Output goes through `io.write_out` / `io.write_err`. Errors are a message plus a non-zero status, never a panic.
4. **Declare the module and add one line to `BUILTINS`** in `builtins/mod.rs`.
5. **`make check`** before committing.

## Planned, roughly in order

`exit`, `true`, `false`, `pwd`, `cd`, then `export`, `unset`, `alias`, `source`, `test`/`[`, then `read`, `printf`, `shift`, `return`, `set`, then `jobs`, `wait`, `type`, `history`.

Coreutils like `ls`, `grep` and `cp` are deliberately **not** planned as builtins for now. Real tools already cover them, and they'd pull focus from the shell itself.
