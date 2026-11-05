//! `export`: mark variables to be passed to programs Reef starts.

use crate::shell::{Shell, is_valid_name};
use crate::streams::Io;

/// `export [name[=value]...]` or `export [-p]`
///
/// - `name=value`: set the variable and export it
/// - `name`: export the variable if it's set
/// - no arguments or `-p`: list exported variables as `export name='value'`,
///   sorted, in a form that can be run again
///
/// An invalid name is reported and skipped; the others are still exported,
/// and the status is 1.
pub fn export_cmd(sh: &mut Shell, argv: &[String], io: &mut Io) -> i32 {
    let args = argv.get(1..).unwrap_or(&[]);
    if args.is_empty() || args == ["-p"] {
        return list(sh, io);
    }

    let mut failed = false;
    for arg in args {
        let (name, value) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (arg.as_str(), None),
        };
        if !is_valid_name(name) {
            io.error(format_args!("export: `{arg}': not a valid identifier"));
            failed = true;
            continue;
        }
        match value {
            Some(v) => sh.set_var(name, v.to_string(), true),
            None => {
                // ponytail: an unset name isn't remembered as exported;
                // needed once `name=value` assignments exist
                if let Some(current) = sh.var(name) {
                    sh.set_var(name, current.to_string(), true);
                }
            }
        }
    }
    if failed { 1 } else { 0 }
}

fn list(sh: &Shell, io: &mut Io) -> i32 {
    let mut vars: Vec<(&str, &str)> = sh.exported_vars().collect();
    vars.sort_unstable();
    let out: String = vars
        .iter()
        .map(|(name, value)| format!("export {name}={}\n", single_quote(value)))
        .collect();
    super::print(io, "export", &out)
}

/// `value` in single quotes, so the shell reads it back unchanged:
/// `it's` becomes `'it'\''s'`.
fn single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}
