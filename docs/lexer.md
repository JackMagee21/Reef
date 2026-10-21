# The lexer

`src/lexer/mod.rs` turns a line of text into words. `src/word.rs` defines what a word is.

## Words keep their quoting

A word isn't a plain string. It's a list of parts, and each part remembers whether it was quoted:

```
a'b'c   →   one Word:  [ "a" (unquoted), "b" (quoted), "c" (unquoted) ]
```

Today the parts are simply joined (`Word::to_plain`) before running the command. The flag matters later: `$HOME` and `*.rs` will expand when unquoted, but `'$HOME'` and `"*.rs"` must not. RShell turned everything into plain strings straight away, and that's why its `'$HOME'` still expanded.

## The rules

| Input | Rule |
|---|---|
| space, tab, newline | ends the current word, unless inside quotes |
| `'...'` | everything up to the next `'` is literal, no exceptions |
| `"..."` | literal, except that `\` escapes `$`, `` ` ``, `"`, `\` and newline. Any other `\` stays as-is |
| `\` outside quotes | the next character is literal. `\` then newline joins the lines |
| `\` at the very end | stays as a literal `\`, like `bash -c 'echo \'` |
| `#` at the start of a word | the rest of the line is a comment |
| `#` inside a word | an ordinary character |
| pieces with no space between them | join into one word |
| unclosed `'` or `"` | `LexError::Incomplete` |

## Examples

| You type | `echo` receives | Prints |
|---|---|---|
| `echo "hello"` | `hello` | `hello` |
| `echo a'b'c` | `abc` | `abc` |
| `echo "echo hhe"bb""` | `echo hhebb` (one argument) | `echo hhebb` |
| `echo "hello "78" "` | `hello 78 ` (one argument) | `hello 78 ` |
| `echo '''sssa''''54 having it'` | `sssa54 having it` | `sssa54 having it` |
| `echo -n "hello 'j'"` | `-n`, `hello 'j'` | `hello 'j'` with no newline |
| `echo 'a  b'` | `a  b` | `a  b` |
| `echo a\ b` | `a b` (one argument) | `a b` |
| `echo ''` | one empty argument | an empty line |
| `echo a '' b` | `a`, (empty), `b` | `a  b` (two spaces) |
| `echo '$HOME'` | `$HOME` | `$HOME` |
| `echo "a\"b"` | `a"b` | `a"b` |
| `echo "a\b"` | `a\b` | `a\b` |
| `echo hi # note` | `hi` | `hi` |
| `echo a#b` | `a#b` | `a#b` |
| `echo "hello '6' ""` | | `reef: syntax error: unexpected end of input: unclosed "` (status 2) |

Quote pairs are matched left to right, so `''` is an empty piece, not an escaped quote.

## Incomplete input

An unclosed quote returns `LexError::Incomplete` rather than a general error. Today Reef reports it as a syntax error with status 2. Once the parser exists, the interactive prompt will use it to show a continuation prompt and read another line instead (**planned**). With `-c` it stays an error, like `bash -c`.

## What the lexer doesn't do yet (planned)

- **Operators:** `|`, `||`, `&`, `&&`, `;`, `>`, `>>`, `<`, `(`, `)`. Today they're ordinary characters, so `a|b` is one word.
- **`$` forms:** `$x`, `${x}`, `$(...)`, `$((...))`. These become new kinds of `WordPart`.
- **Spans:** the position of each word in the input, for error messages and highlighting.
- **Heredocs:** `<<EOF`.

Globs (`*`, `?`) and `~` are handled by the expander, not the lexer, so they stay ordinary characters here on purpose.

## Design rules

- **No I/O, no `Shell`, no OS code.** Text in, words out, so it's easy to test and the highlighter can reuse it.
- **Walk characters, never slice by byte position.** Non-ASCII text like `échø` stays intact.
- **Never panic on input.** Anything unexpected is a `LexError`.
