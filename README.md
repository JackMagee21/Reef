# Reef

The prodigal son of RShell: a POSIX-style shell written in Rust, native on Windows and Linux.

**Status: 0.3, an early preview.** Good for trying out, not yet for daily use or scripts you depend on.

## Install

You need [Rust](https://rustup.rs) 1.85 or newer. This puts `reef` in `~/.local/bin`, which most Linux systems already have on `PATH`:

```sh
cargo install --git https://github.com/JackMagee21/Reef --root ~/.local
```

On Windows, leave out `--root ~/.local`; it then goes in `%USERPROFILE%\.cargo\bin`, which the Rust installer adds to `PATH`.

```sh
reef                      # interactive prompt: jmt@link:~$
reef -c 'echo hello'      # run one command
echo 'echo hi' | reef     # run commands from stdin
reef --version
```

Or from a clone: `cargo run`, or `make run`.

## Update

```sh
reef --check-update   # is there a newer release? (status 0: no, 1: yes)
reef --update         # install it
reef --rollback       # changed your mind? back to the version before
```

Updates only ever install a **release** (a tag like `v0.4.0`), never unreleased work in progress, and never a version older than the one you have. `--check-update` asks GitHub which releases exist, without downloading anything (it needs `git`). `--update` builds the newest one with `cargo install --locked`, so its dependencies are exactly the ones it was tested with, and replaces the installed `reef` wherever it was installed. If you already have it, nothing is rebuilt.

Each update keeps the version it replaced as `reef.previous`, next to `reef`, and only once the new one is installed, so a failed update never costs you the old copy. `reef --rollback` swaps the two; run it again to switch forward. If Reef itself won't start, rename `reef.previous` to `reef` by hand (from bash, Ctrl+Shift+B in Konsole). Works from inside Reef too; the new version is used the next time you start `reef`.

Nothing is sent anywhere except the requests to GitHub that fetch the release.

To make a release: set `version` in `Cargo.toml`, commit, then `git tag -a v0.4.0 -m "reef 0.4.0"` and `git push --follow-tags`.

## Use it as your terminal's shell

Point your terminal at Reef rather than changing your login shell: Reef can't run script files yet, and other programs (ssh, editors, IDE terminals) run commands through your login shell.

- **GNOME Terminal / Ptyxis:** Preferences → Profiles → add a profile → *Use a custom command* → `reef` (or the full path, e.g. `/home/you/.local/bin/reef`).
- **Windows Terminal:** Settings → *Add a new profile* → Command line: `reef.exe`.
- **Just for now:** run `reef` in any terminal; `exit` takes you back.

## What works

- Commands and programs on `PATH` (with `PATHEXT` on Windows)
- Quoting: `'...'`, `"..."`, `\`
- Variables: `x=1`, `$x`, `${x}`, `$?`, `VAR=value program`, with field splitting like `sh`
- Command substitution: `cd $(git rev-parse --show-toplevel)`, `x=$(ls | wc -l)`
- Pipes: `ls | grep x | wc -l`
- `~` and globs: `cd ~/src`, `ls *.rs`, `rm src/*/*.tmp`, `[a-z]`, `?`
- `;`, `&&`, `||`, `!`, `{ ...; }` groups and `( ... )` subshells
- Redirects: `>`, `>>`, `<`, `<>`, `>|`, `2>&1`, `2>/dev/null` (also on Windows)
- Builtins: `cd`, `echo`, `exit`, `export`, `false`, `pwd`, `true`, `type`, `unset`
- Lines ending in `\`, `&&` or an open quote continue on the next line
- History: ↑/↓ go through earlier commands, saved between sessions in `~/.local/share/reef/history` (`%APPDATA%\reef\history` on Windows)
- Suggestions: as you type, a faded match from your history appears; → accepts it
- Tab completion: the first word to a command, the rest to files and folders (`cat src/pa` → `src/parser/`). One match fills in, several open a menu

## Not yet

Background jobs (`&`, `fg`, `jobs`), aliases, `read`, backticks, heredocs, `if`/`for`/`while`/`case`, functions, running script files. Anything not supported says so (`reef: not supported yet: ...`) instead of doing the wrong thing.

## More

[docs/](docs/README.md) explains how Reef works inside, and [docs/design/0001-goals.md](docs/design/0001-goals.md) explains why.
