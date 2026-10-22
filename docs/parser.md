# The parser and running commands

`src/parser/mod.rs` turns the lexer's tokens into a tree of commands, defined in `src/ast.rs`. `src/exec.rs` walks that tree and runs it.

```
echo a && echo b; (cd x)

List
├── Item: AndOr   [echo a]  &&  [echo b]
└── Item: AndOr   Subshell( List: [cd x] )
```

## The grammar so far

A subset of the POSIX grammar. `?` means optional and `*` means repeated.

```
list      := newline* (and_or (sep newline*)?)*         sep is `;`, `&` or a newline
and_or    := pipeline ((`&&` | `||`) newline* pipeline)*
pipeline  := `!`? command (`|` newline* command)*
command   := `(` list `)` redirect* | `{` list `}` redirect* | simple
simple    := (word | redirect)+
redirect  := io_number? redirect_op word
```

Each rule is one function in the parser (recursive descent), so the code reads like the grammar. Adding `if` or `while` later means adding a rule and a `Command` variant, not restructuring.

| Rule | Tree type | Example |
|---|---|---|
| `list` | `List` (a `Vec<Item>`) | `a; b & c` |
| `and_or` | `AndOr` | `a && b \|\| c` |
| `pipeline` | `Pipeline` | `! a \| b` |
| `command` | `Command::Simple`, `Subshell`, `Group` | `echo hi`, `( a )`, `{ a; }` |
| `redirect` | `Redirect` | `2>&1`, `> out` |

Details that are easy to get wrong:

- **Newlines are allowed after `&&`, `||` and `|`.** `a &&` followed by `b` on the next line is one command.
- **Redirects can go anywhere in a simple command.** `> out echo hi` is the same as `echo hi > out`.
- **`{` and `}` are reserved words, not operators.** They only count at the start of a command and unquoted, so `echo { }` prints `{ }`. That's why a group needs `;` before `}`: in `{ echo a }`, the `}` is an argument to `echo`.
- **`( )` and `{ }` can't be empty.** POSIX requires at least one command inside.
- **`if`, `then`, `while`, `for`, `case` and the rest** are a "not supported yet" error at the start of a command, instead of being run as a program called `if`. Elsewhere they're ordinary words (`echo if` prints `if`).

## Errors

| Kind | When | What happens |
|---|---|---|
| `ParseError::Incomplete` | the input stops in the middle: `echo a &&`, `{ echo a;`, an open quote | piped mode and the prompt read another line; `-c` reports it |
| `ParseError::Syntax` | the input is wrong: `; echo`, `echo )`, `echo >` | reported with its position, status 2 |

Syntax errors point at the token: `unexpected `)` (column 8)`, or `(line 2, column 3)` when the input has several lines.

**At the prompt**, pressing Enter on an unfinished command starts a new line instead of running it, so you can type `{`, `echo a` and `}` on three lines. The prompt's line editor asks the parser whether the text is finished.

## Running the tree (`exec.rs`)

| Construct | How it runs |
|---|---|
| `a; b` or a newline | `a`, then `b`. The status is the last command's |
| `a && b` | `b` only if `a` returned 0 |
| `a \|\| b` | `b` only if `a` didn't return 0 |
| `a && b \|\| c` | left to right: `(a && b) \|\| c` |
| `! a` | 0 becomes 1, anything else becomes 0 |
| `{ a; b; }` | in this shell: a `cd` inside changes the directory afterwards |
| `( a; b )` | in a **copy** of the shell (`Shell::clone`): `cd`, variables and `exit` inside don't leak out. This is Reef's replacement for `fork` |
| `exit` | stops the rest of the line too: `exit 4; echo no` prints nothing |
| a blank line or a comment | runs nothing and keeps `$?` |

`exit` with no number uses the status of the command just before it, even inside a chain: `false; exit` exits with 1.

### Parsed but not run yet

These parse fully, but each needs new kinds of `Io` stream before it can run, so they're an error for now:

| Input | Message |
|---|---|
| `a \| b` | `pipelines (`\|`) are not supported yet` |
| `echo hi > out`, `2>&1` | `redirects (`>out`) are not supported yet` |
| `sleep 1 &` | `background jobs (`&`) are not supported yet` |

They're checked for the **whole input before anything runs**, so `echo first; echo a | b` prints nothing rather than running half the line.

## Not done yet (planned)

- **Running pipes, redirects and `&`**: new `Stream` kinds in `io.rs` (see [io.md](io.md)).
- **Control flow**: `if`, `while`, `until`, `for`, `case`, and functions.
- **Heredoc bodies**: `<<` parses, but the lines after it aren't read yet.
- **Variable assignments**: `x=1 cmd`.
