# Architecture

How a line of input travels through Reef today, and which file owns each step.

## Ways to start Reef

| Command | What happens | Code |
|---|---|---|
| `reef` in a console | interactive `reef` prompt | `repl.rs` |
| `reef` with piped input (`"echo hi" \| reef`) | each line of stdin runs as a command, like `sh`. A quote left open or a line ending in `\` carries on to the next line. A syntax error stops the script with status 2. Lines are read a byte at a time, so a program one line starts can read the lines after it, as in bash | `repl.rs` |
| `reef -c "command"` | runs that one command and exits with its status | `main.rs`, `exec::run_source` |
| anything else | prints usage, exits with 2 | `main.rs` |

At startup `main.rs` also installs a Ctrl-C handler that does nothing, so Ctrl-C stops the running program but never Reef itself. Once loops and `read` exist, the handler must set a flag they check, or Ctrl-C could never stop them.

`Shell::new` imports the environment (skipping any variable that isn't valid UTF-8, instead of crashing) and sets `PWD` to the real starting directory, since an inherited `PWD` can be missing or stale.

## How one line runs

```
"cd src && echo 'a  b'"
      │
      ▼
lexer::lex()            src/lexer/mod.rs    text → tokens (words + operators, with spans)
      │
      ▼
parser::parse()         src/parser/mod.rs   tokens → tree (List, AndOr, Pipeline, Command)
      │                                      unfinished → more lines; wrong → syntax error, 2
      ▼
check_supported()       src/exec/mod.rs     pipes, `&`, heredocs → "not supported yet"
      │                                      (checked before anything runs)
      ▼
exec::list()            src/exec/mod.rs     walk the tree: `;` `&&` `||` `!` `{ }` `( )`,
      │                                      each command's redirects (exec/redirect.rs)
      │
      ▼  each simple command
Word::joined_text()     src/word.rs         Words → plain strings      (until expansion)
      │
      ├── builtin?      src/builtins/       call it with (Shell, args, Io)
      │
      └── otherwise     src/platform/       find it on PATH, start it as a process
      │
      ▼
exit status (i32)  →  Shell::last_status  ($?)
```

`exec::run_source()` ties it together: parse, check, run. The prompt, piped mode and `-c` all go through it. See [parser.md](parser.md) for the grammar and how each construct runs.

## The pieces

| File | Owns | Depends on the OS? |
|---|---|---|
| `main.rs` | startup and argument handling only | no |
| `status.rs` | the exit statuses with a fixed meaning (`SYNTAX_ERROR`, `NOT_FOUND`...) and `negate` for `!` | no |
| `parser/mod.rs` | `parse()`: tokens into a tree, with incomplete and syntax errors | no |
| `ast.rs` | the tree types: `List`, `Item`, `AndOr`, `Pipeline`, `Command`, `Redirect` | no |
| `exec/mod.rs` | `run_source` (parse, check, run), walking the tree, and starting builtins and programs | no |
| `exec/redirect.rs` | turning a command's redirects into the `Io` it runs with | no |
| `repl.rs` | the prompt loop (reedline) and piped-stdin mode | no |
| `shell.rs` | `Shell`: state that survives between commands | no |
| `word.rs` | `Word` and `WordPart`: one argument before expansion | no |
| `lexer/mod.rs` | `lex()`: text into tokens, following POSIX quoting | no |
| `lexer/token.rs` | `Token`, `TokenKind`, `Op`, `Span` | no |
| `builtins/` | commands Reef runs itself, and the one table of them | no |
| `streams.rs` | `Io` and `Stream`: where a command's input and output go | no |
| `platform/` | everything that differs between Windows and Linux | **yes, only here** |

## `Shell`: what survives between commands

Every command gets the same `&mut Shell`. That's how `cd` changes the directory for the next command, and how `$?` knows what the last one returned.

| Field | Holds |
|---|---|
| `cwd` (private) | the shell's current directory, read with `cwd()`. Change it only with `set_cwd()`, which keeps `$PWD` and `$OLDPWD` in step. Reef never calls `set_current_dir`; child programs get this directory explicitly |
| `last_status` | `$?`. Written by `exec` after every pipeline (and when a line can't run at all), and by the prompt for Ctrl-C. Everything else only reads it |
| `stop_requested` | set by `exit`, and by a syntax error when not interactive; the prompt and piped mode stop after the current command |
| `interactive` | true at the prompt, where a syntax error doesn't stop Reef |
| `vars` (private) | variables and whether each is exported, read with `var` and changed only through `set_var` and `unset_var`. Only exported ones reach child programs. Names are case-insensitive on Windows |

More fields (functions, aliases, positional arguments, options) are **planned** and get added when a feature needs them.

`Shell` is `Clone`. Subshells `( ... )` run on a copy, which is Reef's replacement for `fork`; pipeline stages (**planned**) will too.

## How a command is found

1. Is it a builtin? Look it up in the table in `builtins/mod.rs`.
2. Otherwise, search `PATH` with `platform::find_command`. On Windows this tries every `PATHEXT` extension (`.exe`, `.cmd`, `.bat`...). Both come from the shell's variables, not Reef's starting environment.
3. Not found: print `reef: NAME: command not found`, status 127.

External programs start with `Shell::cwd` as their directory, only the exported variables as their environment, and the `Io` streams as their stdin, stdout and stderr.

## Exit statuses

| Status | Meaning |
|---|---|
| 0 | success |
| 1 | general failure (for example a builtin couldn't write its output) |
| 2 | syntax error, or bad usage of `reef` itself |
| 126 | found but couldn't be started (no permission, a directory, a bad `#!` line) |
| 127 | command not found |
| 130 | stopped by Ctrl-C |
| 128 + N | killed by signal N (Linux), or crashed (Windows: access violation → 139). `$?` is always 0-255 |

## Planned, in order

Pipelines → expansion (`$VAR`, `~`, globs) → control flow and functions → editor features (history file, completion, highlighting).
