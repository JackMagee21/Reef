//! Expansion: turning a `Word` into the strings a command actually gets.
//!
//! Unquoted `$x` is split into fields on spaces, tabs and newlines, and an
//! unquoted `$x` that's empty disappears: with `x='a  b'`, `echo $x` gets two
//! arguments, `echo "$x"` one. Quoted parts are never split.

use crate::shell::Shell;
use crate::word::{Word, WordPart};

/// A command argument: zero or more fields.
// shortcut: splits on the default IFS (space, tab, newline) and ignores `$IFS`; read it once someone sets IFS
pub fn fields(word: &Word, sh: &Shell) -> Vec<String> {
    let mut fields = Vec::new();
    // The field being built; `Some` once anything, even `""`, started it
    let mut cur: Option<String> = None;
    for part in &word.parts {
        match part {
            WordPart::Literal { text, .. } => cur.get_or_insert_default().push_str(text),
            WordPart::Param {
                name, quoted: true, ..
            } => cur.get_or_insert_default().push_str(&value(name, sh)),
            WordPart::Param { name, .. } => {
                let value = value(name, sh);
                let is_blank = |c: char| matches!(c, ' ' | '\t' | '\n');
                // Leading blanks end the field before; trailing blanks end this one
                if value.starts_with(is_blank) {
                    fields.extend(cur.take());
                }
                for (i, piece) in value.split(is_blank).filter(|p| !p.is_empty()).enumerate() {
                    if i > 0 {
                        fields.extend(cur.take());
                    }
                    cur.get_or_insert_default().push_str(piece);
                }
                if value.ends_with(is_blank) {
                    fields.extend(cur.take());
                }
            }
        }
    }
    fields.extend(cur);
    fields
}

/// One string, never split: for `x=$y` values and redirect targets.
pub fn single(word: &Word, sh: &Shell) -> String {
    word.parts
        .iter()
        .map(|part| match part {
            WordPart::Literal { text, .. } => text.clone(),
            WordPart::Param { name, .. } => value(name, sh),
        })
        .collect()
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
                TokenKind::Word(w) => fields(w, &sh),
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
        assert_eq!(single(w, &sh), "a  b-3");
    }
}
