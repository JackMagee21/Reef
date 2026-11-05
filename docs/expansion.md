# Expansion and variables

`src/expand.rs` turns a `Word` into the strings a command gets. The lexer has already split `$x`, `${x}` and `$?` out as `WordPart::Param`, each remembering whether it was quoted.

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

Redirect targets and assignment values are expanded but **never split**: `f='my file'; echo hi > $f` writes to `my file`.

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

`~`, globs (`*`, `?`, `[...]`), `$(...)`, `$((...))`, the `${...}` operators, positional arguments, and `$IFS`.
