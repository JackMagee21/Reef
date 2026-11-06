# Expansion and variables

`src/expand.rs` turns a `Word` into the strings a command gets. The lexer has already split `$x`, `${x}` and `$?` out as `WordPart::Param`, each remembering whether it was quoted.

The steps, in order: `~` → `$x` and `$(...)` → field splitting → globs. Quoted text skips splitting and globs.

## `~`

`~` alone or `~/...` at the start of a word becomes the home directory (`$HOME`; on Windows `$USERPROFILE` if `HOME` is unset). It counts as quoted, so a home with spaces or `*` in it is never split or globbed. It also works after `=` in an assignment: `x=~/bin`, `export PATH=~/bin`.

`'~'`, `"~"`, `\~`, `a~` and `~x` are left alone. `~user` (another user's home) is **planned**.

## What expands

| Form | Value |
|---|---|
| `$name`, `${name}` | the variable, or empty if it's unset. A name is a letter or `_`, then letters, digits and `_`. Use braces to end it early: `${x}y` |
| `$?` | the status of the last command |
| `'$x'`, `\$x`, `"\$x"` | not expanded: a literal `$x` |
| `$` before anything else (`echo $`, `a$`) | a literal `$` |

`${x:-default}` and the other `${...}` operators are refused before anything runs (`not supported yet`), and so is `${}`. `$1`, `$#`, `$$`, `$@` and `$0` stay literal until positional arguments exist (**planned**).

## Field splitting

An **unquoted** `$x` is split into separate arguments on spaces, tabs and newlines, and one that expands to nothing disappears. A **quoted** one is always exactly one argument:

| `x` is | `echo [$x]` gets | `echo "[$x]"` gets |
|---|---|---|
| `a   b` | `[a` and `b]` | `[a   b]` |
| unset | `[]` | `[]` |

| `x` is | `printf ... $x` gets | `printf ... "$x"` gets |
|---|---|---|
| `a   b` | 2 arguments | 1 argument |
| unset | 0 arguments | 1 empty argument |

`$IFS` is ignored for now; splitting always uses space, tab and newline.

Redirect targets and assignment values are expanded but **never split or globbed**: `f='my file'; echo hi > $f` writes to `my file`, and `x=*` sets `x` to `*`.

## Globs

After splitting, a field with an **unquoted** `*`, `?` or `[` is a pattern, matched against file names (`src/glob.rs`):

| Pattern | Matches |
|---|---|
| `*` | any run of characters, even none |
| `?` | any one character |
| `[abc]`, `[a-z]` | one character from the set |
| `[!abc]`, `[^abc]` | one character not in it |
| `\*`, `'*'`, `"*"` | a literal `*` |

- Each `/`-separated piece is matched separately, so `*/*.rs` looks one folder down, and `*` never matches a `/`.
- Names starting with `.` only match a pattern that starts with `.`: `*` skips `.git`, `.*` finds it (but never `.` or `..`).
- A pattern ending in `/` matches only folders: `*/`.
- Matches are sorted. **No match keeps the word as typed**, like `sh`: `ls *.none` passes `*.none` to `ls`, which reports it missing.
- An unquoted `$x` is globbed too: with `x='*.txt'`, `echo $x` lists the files and `echo "$x"` prints `*.txt`.
- On Windows matching ignores case (`*.TXT` finds `a.txt`), like the rest of Windows.

`[[:alpha:]]` classes and names that aren't valid UTF-8 are **planned**.

## `$(...)`, command substitution

`$(cmd)` runs `cmd` and is replaced by what it printed, minus **every** trailing newline (and `\r`, for Windows programs):

| Input | Gets |
|---|---|
| `echo "[$(echo hi)]"` | `[hi]` |
| `x=$(ls \| wc -l)` | the count, in `x` |
| `echo $(echo 'a  b')` | `a b`: unquoted, it's split (and globbed) like `$x` |
| `echo "$(echo 'a  b')"` | `a  b`: quoted, it's one argument, untouched |

- It runs in a **copy** of the shell, like `( )`: `cd`, `x=1` and `exit` inside don't leak out.
- Only its stdout is captured. Its errors still reach the screen, and its stdin is the command's.
- With no command left, the status is the substitution's: `x=$(false); echo $?` prints `1`. A later `$?` in the same line sees it too.
- The end is the **matching** `)`: quotes, `\`, nested `( )` and `$( )` and comments are stepped over, so `$(echo ")")` works. An unclosed `$(` is unfinished input: the prompt waits for the next line.
- What's inside is parsed and checked with everything else before anything runs: `echo a; echo $(while x)` prints only the error.

How it runs (`expand::substitute`): a pipe, the commands on a thread writing into it, and Reef reading the other end *while they run*. A pipe holds about 64 KB, so reading only after they finished would hang on bigger output.

Backticks (`` `cmd` ``), the older form, are **planned**; until then a backtick is a plain character. `$((...))` arithmetic is refused as not supported yet.

## Assignments

A word like `name=value` at the start of a simple command is an assignment, if `name` is written plainly (`'x'=1` and `"x=1"` are ordinary words).

| Input | Effect |
|---|---|
| `x=1` | sets the shell variable `x`. It's not exported unless it already was, or `export x` is run |
| `x=1 y=$x` | assignments apply left to right, so `y` is `1` |
| `x=1 program` | `x` is only in `program`'s environment; the shell's own `x` is unchanged |
| `x=1 echo` | before a builtin: refused for now (`not supported yet`) |
| `export x=$y` | `$y` isn't split, like a plain assignment, so `export x=$y` works when `$y` contains spaces |

Variables live in `Shell` (see [architecture.md](architecture.md)). Subshells `( )` get a copy, so assignments inside don't leak out; `{ }` groups share the shell.

## Planned

`~user`, `[[:class:]]` in globs, backticks, `$((...))`, the `${...}` operators, positional arguments, and `$IFS`.
