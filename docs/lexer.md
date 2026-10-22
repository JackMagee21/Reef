# The lexer

`src/lexer/mod.rs` turns text into **tokens**: words and operators, each with its span. `src/lexer/token.rs` defines the token types, and `src/word.rs` defines what a word is.

```
echo hi 2>&1 | wc
  Word(echo) Word(hi) IoNumber(2) Op(>&) Word(1) Op(|) Word(wc)
```

## Tokens

| Kind | What it is |
|---|---|
| `Word` | one argument, with its quoting remembered (below) |
| `Op` | an operator (table below). `\n` is one too, because a newline ends a command like `;` does |
| `IoNumber` | the `2` in `2>file` or `2>&1`: unquoted digits directly before `<` or `>`. `2 >file`, `'2'>file` and `a2>file` are words instead |

Every token has a `Span`, the byte offsets of its text in the input: `src[span.start..span.end]`. Spans are what let an error point at a column, and later what the highlighter colors.

### Operators

The lexer always takes the longest operator it can: `>>` is one operator, not two `>`, and `a>>>b` is `>>` then `>`.

| Operator | Name | | Operator | Name |
|---|---|---|---|---|
| `\n` | `Newline` | | `<` | `Less` |
| `;` | `Semi` | | `>` | `Great` |
| `;;` | `DSemi` | | `>>` | `DGreat` |
| `&` | `Amp` | | `<&` | `LessAnd` |
| `&&` | `AndIf` | | `>&` | `GreatAnd` |
| `\|` | `Pipe` | | `<>` | `LessGreat` |
| `\|\|` | `OrIf` | | `>\|` | `Clobber` |
| `(` | `LParen` | | `<<` | `DLess` (heredoc) |
| `)` | `RParen` | | `<<-` | `DLessDash` (heredoc) |

Quoted or escaped, these are just text: `'a;b'`, `"c|d"` and `e\&f` are ordinary words.

**Reserved words** (`if`, `then`, `{`, `}`, `!`...) are deliberately **not** tokens here. `if` is only special at the start of a command (`echo if` prints `if`), so the parser decides.

### Running operators (for now)

The parser doesn't exist yet, so `run()` still runs one simple command. Any operator in it is reported with its column instead of being run wrongly:

```
reef: syntax error: `&&` (column 8) is not supported yet
```

Newlines at the start or end are fine, since piped mode ends every command with one.

## Words keep their quoting

A word isn't a plain string. It's a list of parts, and each part remembers whether it was quoted:

```
a'b'c   →   one Word:  [ "a" (unquoted), "b" (quoted), "c" (unquoted) ]
```

Today the parts are simply joined (`Word::to_plain`) before running the command. The flag matters later: `$HOME` and `*.rs` will expand when unquoted, but `'$HOME'` and `"*.rs"` must not. RShell turned everything into plain strings straight away, and that's why its `'$HOME'` still expanded.

## The rules

| Input | Rule |
|---|---|
| space, tab | ends the current word, unless inside quotes |
| newline, `;`, `&`, `\|`, `(`, `)`, `<`, `>` | ends the current word and starts an operator, unless quoted or escaped |
| `'...'` | everything up to the next `'` is literal, no exceptions |
| `"..."` | literal, except that `\` escapes `$`, `` ` ``, `"`, `\` and newline. Any other `\` stays as-is |
| `\` outside quotes | the next character is literal. `\` then newline joins the lines |
| `\` at the very end | stays as a literal `\`, like `bash -c 'echo \'` |
| `#` at the start of a word | the rest of the line is a comment, up to (not including) the newline. `a;#b` is `a`, `;` and a comment |
| `#` inside a word | an ordinary character |
| pieces with no space between them | join into one word |
| unclosed `'` or `"` | `LexError::Incomplete` |
| `\` then newline at the very end | `LexError::Incomplete`: the next line is still to come |

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

An unclosed quote, or `\` + newline at the very end, returns `LexError::Incomplete` rather than a general error. It means "there's more to come":

- **Piped mode** reads the next line, adds it on and tries again, so `echo 'a` followed by `b'` prints `a`, newline, `b`, like bash. If the input ends first, it's a syntax error and the script stops with status 2.
- **`-c`** has no next line, so it's a syntax error, like `bash -c`. One known difference: `-c 'echo a \<newline>'` is an error in Reef, while bash prints `a`.
- **The prompt** still reports it as an error. Showing a continuation prompt and reading another line is **planned** with the parser.

## What the lexer doesn't do yet (planned)

- **`$` forms:** `$x`, `${x}`, `$(...)`, `$((...))`. These become new kinds of `WordPart`.
- **Heredoc bodies:** `<<` and `<<-` are recognized, but reading the lines after them comes with the parser.
- **Spans in lex errors:** `LexError::Incomplete` doesn't say where the unclosed quote started yet.

Globs (`*`, `?`) and `~` are handled by the expander, not the lexer, so they stay ordinary characters here on purpose.

## Design rules

- **No I/O, no `Shell`, no OS code.** Text in, tokens out, so it's easy to test and the highlighter can reuse it.
- **Walk characters, never slice by guessed byte positions.** Spans come from `char_indices`, so they always land on character boundaries and non-ASCII text like `échø` stays intact.
- **Never panic on input.** Anything unexpected is a `LexError`.
