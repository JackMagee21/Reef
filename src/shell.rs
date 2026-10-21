use std::collections::HashMap;
use std::path::PathBuf;

use crate::platform;

/// Everything that has to survive from one command to the next.
/// Fields get added here as features need them (functions, aliases, positional args, options).
#[derive(Clone)]
pub struct Shell {
    /// The shell's own directory. Reef never calls `set_current_dir`;
    /// children get this via `Command::current_dir`.
    pub cwd: PathBuf,
    /// `$?`
    pub last_status: i32,
    /// Private so every write goes through `set_var` and its rules.
    vars: HashMap<String, Var>,
}

#[derive(Clone)]
struct Var {
    value: String,
    /// Only exported variables reach child processes.
    exported: bool,
}

impl Shell {
    /// Real startup: imports the process environment and current directory.
    // No Default impl on purpose: a default Shell would silently read the real environment.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mut sh = Shell {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            last_status: 0,
            vars: HashMap::new(),
        };
        for (k, v) in std::env::vars() {
            sh.set_var(&k, v, true);
        }
        sh
    }

    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(&platform::var_key(name))
            .map(|v| v.value.as_str())
    }

    /// Set a variable. Once exported it stays exported, like `export` in POSIX sh.
    pub fn set_var(&mut self, name: &str, value: String, export: bool) {
        let key = platform::var_key(name);
        let exported = export || self.vars.get(&key).is_some_and(|v| v.exported);
        self.vars.insert(key, Var { value, exported });
    }

    pub fn exported_vars(&self) -> impl Iterator<Item = (&str, &str)> {
        self.vars
            .iter()
            .filter(|(_, v)| v.exported)
            .map(|(k, v)| (k.as_str(), v.value.as_str()))
    }
}
