# Architecture

How a line of input travels through Reef today, and which file owns each step.

## Ways to start Reef

| Command | What happens | Code |
|---|---|---|
| `reef` in a console | interactive `reef` prompt | `repl.rs` |
| `reef` with piped input (`"echo hi" \| reef`) | each line of stdin runs as a command, like `sh`. A quote left open or a line ending in `\` carries on to the next line. A syntax error stops the script with status 2. Lines are read a byte at a time, so a program one line starts can read the lines after it, as in bash | `repl.rs` |
| `reef -c "command"` | runs that one command and exits with its status | `main.rs` |
| anything else | prints usage, exits with 2 | `main.rs` |

At startup `main.rs` also installs a Ctrl-C handler that does nothing, so Ctrl-C stops the running program but never Reef itself. Once loops and `read` exist, the handler must set a flag they check, or Ctrl-C could never stop them.

`Shell::new` imports the environment (skipping any variable that isn't valid UTF-8, instead of crashing) and sets `PWD` to the real starting directory, since an inherited `PWD` can be missing or stale.

## How one line runs

```
"echo 'a  b' c"
      │
      ▼
lexer::lex()            src/lexer/mod.rs   text → Words, quoting remembered
      │                                     unclosed quote → syntax error, status 2
      ▼
Word::to_plain()        src/word.rs        Words → plain strings      (temporary)
      │
      ▼
run()                   src/main.rs        ["echo", "a  b", "c"]
      │
      ├── builtin?      src/builtins/      call it with (Shell, args, Io)
      │
      └── otherwise     src/platform/      find it on PATH, start it as a process
      │
      ▼
exit status (i32)  →  Shell::last_status  ($?)
```

`run()` in `main.rs` is a placeholder: it treats one line as one simple command. The parser (**planned**) will replace it with a tree of commands joined by `|`, `&&`, `;` and so on.

## The pieces

| File | Owns | Depends on the OS? |
|---|---|---|
| `main.rs` | startup, argument handling, `run()` | no |
| `repl.rs` | the prompt loop (reedline) and piped-stdin mode | no |
| `shell.rs` | `Shell`: state that survives between commands | no |
| `word.rs` | `Word` and `WordPart`: one argument before expansion | no |
| `lexer/mod.rs` | `lex()`: text into words, following POSIX quoting | no |
| `builtins/` | commands Reef runs itself, and the one table of them | no |
| `io.rs` | `Io` and `Stream`: where a command's input and output go | no |
| `platform/` | everything that differs between Windows and Linux | **yes, only here** |

## `Shell`: what survives between commands

Every command gets the same `&mut Shell`. That's how `cd` changes the directory for the next command, and how `$?` knows what the last one returned.

| Field | Holds |
|---|---|
| `cwd` | the shell's current directory. Reef never calls `set_current_dir`; child programs get this directory explicitly |
| `last_status` | the last exit status (`$?`) |
| `exit_requested` | set by `exit`, and by a syntax error when not interactive; the REPL and piped mode stop after the current command |
| `interactive` | true at the prompt, where a syntax error doesn't stop Reef |
| `vars` (private) | variables and whether each is exported. Only exported ones reach child programs. Names are case-insensitive on Windows |

More fields (functions, aliases, positional arguments, options) are **planned** and get added when a feature needs them.

`Shell` is `Clone`. Pipeline stages and subshells (**planned**) will run on a copy, which is Reef's replacement for `fork`.

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

Operators and spans in the lexer → the parser (`|`, `&&`, `;`, redirects) → expansion (`$VAR`, `~`, globs) → pipelines → control flow and functions → editor features (history file, completion, highlighting).
