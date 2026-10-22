# Testing

## Running tests

| Command | Does |
|---|---|
| `make test` (or `cargo test`) | runs every test |
| `cargo test lexer` | only tests whose name contains `lexer` |
| `cargo test --test spec` | only `tests/spec.rs` |
| `cargo test -- --nocapture` | also shows what passing tests print |
| `make check` | everything CI runs: format check, clippy for Windows **and** Linux, then all tests (including the `src/platform/` rule) |

Run `make check` before every commit. If it passes, CI should too.

## The two kinds of tests

### Spec tests: `tests/spec.rs`

These run the real `reef` program, the way a user would.

- **The `CASES` table.** Each row is an input, the exact expected stdout and the expected exit status. Every row runs as `reef -c "input"`. When cases fail, all of them are listed at once with `want` and `got`.
- **The `ERRORS` table.** Each row is an input, the exact expected stderr and the exit status, so error messages are tested too.
- **A few separate tests** for things the tables can't express: unknown arguments exit with 2, piped stdin runs line by line, piped mode exits with the last command's status, `exit` stops reading, `cd`/`pwd` keep their directory between lines, a program run from piped stdin can read the lines after it, quotes and `\` can span lines, and a syntax error stops a script. Some only run on Linux (no execute bit → 126, empty `PATH` entries).

### Layout tests: `tests/layout.rs`

Rules about the source tree itself. Today: OS-specific `cfg` only inside `src/platform/`.

### Unit tests: inside `src/`

At the bottom of a source file, in a `#[cfg(test)] mod tests` block. They test one piece directly, without running Reef. Example: `src/lexer/mod.rs` checks the exact word parts and their quoted flags, which spec tests can't see. `src/repl.rs` tests what the prompt does with each line-editor event (a command, `exit`, Ctrl-C, Ctrl-D, a syntax error) without needing a real terminal; only the terminal drawing itself is checked by hand.

## Rules

- **Write the test before the code.** Add the new `CASES` rows, watch them fail, then implement.
- **Never delete a passing case.** The table is the regression net. If behavior changes on purpose, change the expected value and say why in the commit.
- **Only use commands that behave the same on Windows and Linux** in `CASES`: Reef's own builtins, not `ls` or `dir`. Builtins are looked up before `PATH`, so `echo hi` always runs Reef's own `echo`, even where Git for Windows puts an `echo.exe` on `PATH`.
- **Cases waiting for a feature stay in the table, commented out**, with a note saying what they wait for.
- **When you test something by hand and it works, add it as a case.**

## Checking behavior against a reference shell

When unsure what the right output is, ask a real POSIX shell:

- **Linux:** `dash -c 'your input'; echo $?`
- **Windows:** `busybox sh -c "your input"` (busybox-w32, from frippery.org)

Reef should match them.

## CI

`.github/workflows/ci.yml` runs on every push and pull request, on `windows-latest` and `ubuntu-latest`:

1. `cargo fmt --check`
2. `cargo clippy --all-targets -- -D warnings` (any warning fails)
3. `cargo test`, which includes the check that OS-specific `cfg` only appears in `src/platform/`

The Linux job is the only place the Linux tests actually run. Locally on Windows, `make check` can only compile and lint for Linux.

## Known local issue

Windows Smart App Control can block freshly built, unsigned programs, including `reef.exe` and cargo's build helpers ("An Application Control policy has blocked this file"). It can't exempt a folder, so the options are turning it off or relying on CI. See Windows Security → App & browser control.
