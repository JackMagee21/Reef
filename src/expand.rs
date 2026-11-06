//! Expansion: turning a `Word` into the strings a command actually gets.
//!
//! In order: `~` becomes the home directory; `$x` its value and `$(...)` its
//! output; an unquoted `$x` or `$(...)` is split into fields on spaces, tabs
//! and newlines (and disappears if empty); then each field with an unquoted
//! `*`, `?` or `[` becomes the matching file names, if there are any. Quoted
//! parts are never split or globbed: with `x='a  b'`, `echo $x` gets two
//! arguments, `echo "$x"` one.
//!
//! Expanding can run commands (`$(...)`), so it takes the shell and the
//! streams, and `$(...)` leaves its status in `$?`.

use std::borrow::Cow;
use std::io::Read;

use crate::exec;
use crate::glob;
use crate::platform;
use crate::shell::Shell;
use crate::streams::{Io, Stream, describe_error};
use crate::word::{Word, WordPart};

/// A field being built: its text, and the same text as a glob pattern, with
/// quoted `*?[\` escaped so they only match themselves.
#[derive(Default)]
struct Field {
    text: String,
    pattern: String,
}

impl Field {
    fn push(&mut self, s: &str, quoted: bool) {
        self.text.push_str(s);
        for c in s.chars() {
            if quoted && matches!(c, '*' | '?' | '[' | '\\') {
                self.pattern.push('\\');
            }
            self.pattern.push(c);
        }
    }

    /// The file names it matches, or just its text if it matches none.
    fn glob(self, cwd: &std::path::Path) -> Vec<String> {
        if !glob::is_pattern(&self.pattern) {
            return vec![self.text];
        }
        match glob::expand(&self.pattern, cwd) {
            paths if paths.is_empty() => vec![self.text],
            paths => paths,
        }
    }
}

/// A command argument: zero or more fields.
// shortcut: splits on the default IFS (space, tab, newline) and ignores `$IFS`; read it once someone sets IFS
pub fn fields(word: &Word, sh: &mut Shell, io: &mut Io) -> Vec<String> {
    let mut fields = Vec::new();
    // The field being built; `Some` once anything, even `""`, started it
    let mut cur: Option<Field> = None;
    for part in &tilde(word, sh).parts {
        let (value, quoted) = match part {
            WordPart::Literal { text, quoted } => {
                cur.get_or_insert_default().push(text, *quoted);
                continue;
            }
            WordPart::Param { name, quoted, .. } => (value(name, sh), *quoted),
            WordPart::CommandSub { src, quoted } => (substitute(src, sh, io), *quoted),
        };
        if quoted {
            cur.get_or_insert_default().push(&value, true);
            continue;
        }
        let is_blank = |c: char| matches!(c, ' ' | '\t' | '\n');
        // Leading blanks end the field before; trailing blanks end this one
        if value.starts_with(is_blank) {
            fields.extend(cur.take());
        }
        for (i, piece) in value.split(is_blank).filter(|p| !p.is_empty()).enumerate() {
            if i > 0 {
                fields.extend(cur.take());
            }
            cur.get_or_insert_default().push(piece, false);
        }
        if value.ends_with(is_blank) {
            fields.extend(cur.take());
        }
    }
    fields.extend(cur);
    fields.into_iter().flat_map(|f| f.glob(sh.cwd())).collect()
}

/// One string, never split or globbed: for `x=$y` values and redirect targets.
pub fn single(word: &Word, sh: &mut Shell, io: &mut Io) -> String {
    let mut text = String::new();
    for part in &tilde(word, sh).parts {
        match part {
            WordPart::Literal { text: literal, .. } => text.push_str(literal),
            WordPart::Param { name, .. } => text.push_str(&value(name, sh)),
            WordPart::CommandSub { src, .. } => text.push_str(&substitute(src, sh, io)),
        }
    }
    text
}

/// `$(src)`: runs `src` in a copy of the shell, like `( )`, and returns what
/// it printed, minus trailing newlines. Its status goes in `$?`. Its stdin
/// and stderr are the command's own, so errors still reach the screen.
fn substitute(src: &str, sh: &mut Shell, io: &mut Io) -> String {
    let (mut reader, writer) = match std::io::pipe() {
        Ok(ends) => ends,
        Err(e) => {
            io.error(format_args!("$(...): {}", describe_error(&e)));
            return String::new();
        }
    };
    let mut inner = match io.try_clone() {
        Ok(inner) => inner,
        Err(e) => {
            io.error(format_args!("$(...): {}", describe_error(&e)));
            return String::new();
        }
    };
    inner.stdout = Stream::PipeOut(writer);
    let mut copy = sh.clone();
    // Read while it runs: a pipe only holds ~64 KB, so waiting for it to
    // finish first would hang on bigger output. The thread owns the only
    // writing end, so the read ends when the commands do.
    let (output, status) = std::thread::scope(|scope| {
        let run = scope.spawn(move || exec::run_source(src, &mut copy, &mut inner));
        let mut output = Vec::new();
        let read = reader.read_to_end(&mut output);
        // A thread only fails by panicking, which has already printed why
        (read.map(|_| output), run.join().unwrap_or(1))
    });
    sh.last_status = status;
    let output = match output {
        Ok(output) => output,
        Err(e) => {
            io.error(format_args!("$(...): {}", describe_error(&e)));
            return String::new();
        }
    };
    // `\r` too: Windows programs end lines with `\r\n`
    String::from_utf8_lossy(&output)
        .trim_end_matches(['\n', '\r'])
        .to_string()
}

/// `~` or `~/...` at the start of `word` becomes the home directory, as if
/// quoted, so it's never split or globbed. `~x`, `'~'` and `a~` stay as they are.
// shortcut: `~user` stays literal; it needs the user database
fn tilde<'a>(word: &'a Word, sh: &Shell) -> Cow<'a, Word> {
    if let Some((
        WordPart::Literal {
            text,
            quoted: false,
        },
        rest,
    )) = word.parts.split_first()
        && (text == "~" && rest.is_empty() || text.starts_with("~/"))
        && let Some(home) = sh.first_var(platform::HOME_VARS)
    {
        let mut expanded = Word::default();
        expanded.push_str(home, true);
        expanded.push_str(&text[1..], false);
        expanded.parts.extend_from_slice(rest);
        return Cow::Owned(expanded);
    }
    Cow::Borrowed(word)
}

/// `$?` is the last status; an unset variable is empty.
fn value(name: &str, sh: &Shell) -> String {
    match name {
        "?" => sh.last_status.to_string(),
        _ => sh.var(name).unwrap_or_default().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{TokenKind, lex};

    /// The fields of the one word in `src`, with `x` set to `x_value`.
    fn expand(src: &str, x_value: &str) -> Vec<String> {
        let mut sh = Shell::new();
        sh.set_var("x", x_value.into(), false);
        sh.unset_var("reef_unset");
        match &lex(src).unwrap()[..] {
            [t] => match &t.kind {
                TokenKind::Word(w) => fields(w, &mut sh, &mut Io::terminal()),
                k => panic!("{k:?}"),
            },
            ts => panic!("{ts:?}"),
        }
    }

    #[test]
    fn unquoted_params_split_on_blanks() {
        assert_eq!(expand("$x", "a  b\tc"), ["a", "b", "c"]);
        assert_eq!(expand("pre${x}post", " a  b "), ["pre", "a", "b", "post"]);
        assert_eq!(expand("pre${x}post", "a b"), ["prea", "bpost"]);
        assert_eq!(expand("$x$x", "a b"), ["a", "ba", "b"]);
    }

    #[test]
    fn quoted_params_are_one_field() {
        assert_eq!(expand(r#""$x""#, " a  b "), [" a  b "]);
        assert_eq!(expand(r#"a"$x"b"#, "1 2"), ["a1 2b"]);
    }

    #[test]
    fn empty_unquoted_params_disappear_but_quoted_ones_stay() {
        assert_eq!(expand("$reef_unset", ""), Vec::<String>::new());
        assert_eq!(expand("$x", "   "), Vec::<String>::new());
        assert_eq!(expand(r#""$reef_unset""#, ""), [""]);
        assert_eq!(expand("''$reef_unset", ""), [""]);
        assert_eq!(expand("a$reef_unset", ""), ["a"]);
    }

    #[test]
    fn single_never_splits() {
        let mut sh = Shell::new();
        sh.set_var("x", "a  b".into(), false);
        sh.last_status = 3;
        let TokenKind::Word(w) = &lex("$x-$?").unwrap()[0].kind else {
            panic!()
        };
        assert_eq!(single(w, &mut sh, &mut Io::terminal()), "a  b-3");
    }
}
