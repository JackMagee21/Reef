use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::platform;

/// Everything that has to survive from one command to the next.
/// Fields get added here as features need them (functions, aliases, positional args, options).
#[derive(Clone)]
pub struct Shell {
    /// The shell's own directory. Reef never calls `set_current_dir`;
    /// children get this via `Command::current_dir`. Private so every change
    /// goes through `set_cwd`, which keeps `$PWD` and `$OLDPWD` in step.
    cwd: PathBuf,
    /// `$?`. Written by `exec` after every pipeline (and when a line can't run
    /// at all), and by the prompt for Ctrl-C. Everything else only reads it.
    pub last_status: i32,
    /// Reef should stop after the current command: set by `exit`, and by a
    /// syntax error in a script. Builtins never call `process::exit` themselves.
    pub stop_requested: bool,
    /// True at the prompt. A syntax error stops a script or `-c`, but not the prompt.
    pub interactive: bool,
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
            cwd: platform::clean_path(
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            ),
            last_status: 0,
            stop_requested: false,
            interactive: false,
            vars: HashMap::new(),
        };
        // vars_os, not vars: vars() panics on a single non-UTF-8 variable
        // ponytail: non-UTF-8 variables are dropped, so children don't get them; store OsString if that matters
        for (k, v) in std::env::vars_os() {
            if let (Ok(k), Ok(v)) = (k.into_string(), v.into_string()) {
                sh.set_var(&k, v, true);
            }
        }
        // The inherited PWD may be missing (cmd.exe) or stale (another shell's)
        let pwd = sh.cwd.display().to_string();
        sh.set_var("PWD", pwd, true);
        sh
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Change the shell's directory, updating `$OLDPWD` and `$PWD` with it
    /// (programs read `$PWD`, and `cd -` reads `$OLDPWD`).
    pub fn set_cwd(&mut self, new: PathBuf) {
        let old = std::mem::replace(&mut self.cwd, new);
        self.set_var("OLDPWD", old.display().to_string(), true);
        self.set_var("PWD", self.cwd.display().to_string(), true);
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

    pub fn unset_var(&mut self, name: &str) {
        self.vars.remove(&platform::var_key(name));
    }
}
