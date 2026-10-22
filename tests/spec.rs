use assert_cmd::Command;

// (input, expected stdout, expected exit code)
// Add every new feature's cases here BEFORE implementing it, and never delete a passing case.
// Only use inputs that behave the same on Windows and Linux (Reef's own builtins, not ls/dir).
const CASES: &[(&str, &str, i32)] = &[
    ("", "", 0),
    ("   ", "", 0),
    ("nosuchcmd_reef_test", "", 127),
    ("echo hi", "hi\n", 0),
    ("echo", "\n", 0),
    ("echo a b c", "a b c\n", 0),
    ("echo -n", "", 0),
    ("echo -n abc", "abc", 0),
    ("echo hi -n", "hi -n\n", 0),
    // Quoting (lexer)
    ("echo a'b'c", "abc\n", 0),
    ("echo \"hello\"", "hello\n", 0),
    ("echo \"echo hhe\"bb\"\"", "echo hhebb\n", 0),
    ("echo -n \"hello 'j'\"", "hello 'j'", 0),
    ("echo 'a  b'", "a  b\n", 0),
    ("echo a\\ b", "a b\n", 0),
    ("echo ''", "\n", 0),
    ("echo a '' b", "a  b\n", 0),
    ("echo '$HOME'", "$HOME\n", 0),
    ("echo \"a\\\"b\"", "a\"b\n", 0),
    ("echo \"a\\b\"", "a\\b\n", 0),
    ("echo \"it's\"", "it's\n", 0),
    ("echo hi # a comment", "hi\n", 0),
    ("echo a#b", "a#b\n", 0),
    ("# only a comment", "", 0),
    ("echo \"hello \"78\" \"", "hello 78 \n", 0),
    ("echo '''sssa''''54 having it'", "sssa54 having it\n", 0),
    // Unclosed quotes are a syntax error for now (multi-line input comes with the parser)
    ("echo 'a", "", 2),
    ("echo \"a", "", 2),
    ("echo \"hello '6' \"\"", "", 2),
    // true / false
    ("true", "", 0),
    ("true ignores args", "", 0),
    ("false", "", 1),
    // exit
    ("exit", "", 0),
    ("exit 3", "", 3),
    ("exit 300", "", 44),
    ("exit -1", "", 255),
    ("exit abc", "", 2),
    ("exit 1 2", "", 1),
    // cd errors (cd + pwd together need piped stdin, see below)
    ("cd nosuchdir_reef_test", "", 1),
    ("cd a b", "", 1),
    ("cd Cargo.toml", "", 1),
];

#[test]
fn spec() {
    let mut failed = vec![];
    for (src, out, code) in CASES {
        let o = Command::cargo_bin("reef")
            .unwrap()
            .args(["-c", src])
            .output()
            .unwrap();
        let got_out = String::from_utf8_lossy(&o.stdout);
        let got_code = o.status.code().unwrap_or(-1);
        if (got_out.as_ref(), got_code) != (*out, *code) {
            failed.push(format!(
                "{src:?}\n  want {out:?} {code}\n  got  {got_out:?} {got_code}"
            ));
        }
    }
    assert!(failed.is_empty(), "\n{}", failed.join("\n"));
}

// (input, expected stderr, expected exit code): the exact error messages.
// CASES only checks stdout, so a missing or wrong message would pass there.
const ERRORS: &[(&str, &str, i32)] = &[
    ("echo ok", "", 0),
    (
        "nosuchcmd_reef_test",
        "reef: nosuchcmd_reef_test: command not found\n",
        127,
    ),
    (
        "echo 'a",
        "reef: syntax error: unexpected end of input: unclosed '\n",
        2,
    ),
    (
        "echo \"a",
        "reef: syntax error: unexpected end of input: unclosed \"\n",
        2,
    ),
    (
        "cd nosuchdir_reef_test",
        "reef: cd: nosuchdir_reef_test: No such file or directory\n",
        1,
    ),
    ("cd a b", "reef: cd: too many arguments\n", 1),
    (
        "cd Cargo.toml",
        "reef: cd: Cargo.toml: Not a directory\n",
        1,
    ),
    (
        "exit abc",
        "reef: exit: abc: numeric argument required\n",
        2,
    ),
    ("exit 1 2", "reef: exit: too many arguments\n", 1),
];

#[test]
fn error_messages() {
    let mut failed = vec![];
    for (src, err, code) in ERRORS {
        let o = Command::cargo_bin("reef")
            .unwrap()
            .args(["-c", src])
            .output()
            .unwrap();
        let got_err = String::from_utf8_lossy(&o.stderr);
        let got_code = o.status.code().unwrap_or(-1);
        if (got_err.as_ref(), got_code) != (*err, *code) {
            failed.push(format!(
                "{src:?}\n  want {err:?} {code}\n  got  {got_err:?} {got_code}"
            ));
        }
    }
    assert!(failed.is_empty(), "\n{}", failed.join("\n"));
}

#[test]
fn piped_quotes_can_span_lines() {
    Command::cargo_bin("reef")
        .unwrap()
        .write_stdin("echo 'a\nb'\necho next\n")
        .assert()
        .success()
        .stdout("a\nb\nnext\n");
}

#[test]
fn piped_backslash_newline_joins_lines() {
    Command::cargo_bin("reef")
        .unwrap()
        .write_stdin("echo a \\\nb\n")
        .assert()
        .success()
        .stdout("a b\n");
}

/// POSIX: a syntax error stops a script, it doesn't skip to the next line.
#[test]
fn piped_syntax_error_stops_the_script() {
    Command::cargo_bin("reef")
        .unwrap()
        .write_stdin("echo one\necho 'never closed\necho three\n")
        .assert()
        .code(2)
        .stdout("one\n");
}

#[cfg(unix)]
#[test]
fn a_file_that_cant_run_is_126_not_127() {
    // Cargo.toml exists but has no execute bit
    Command::cargo_bin("reef")
        .unwrap()
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["-c", "./Cargo.toml"])
        .assert()
        .code(126);
}

#[cfg(unix)]
#[test]
fn empty_path_entry_means_the_shells_dir() {
    // An empty PATH entry means "the current directory", which must be the one
    // Reef `cd`ed into, not the one it started in.
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("reef_path_test");
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("reef_hello");
    std::fs::write(&script, "#!/bin/sh\necho hello\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    Command::cargo_bin("reef")
        .unwrap()
        .env("PATH", ":/usr/bin:/bin")
        .write_stdin(format!("cd '{}'\nreef_hello\n", dir.display()))
        .assert()
        .success()
        .stdout("hello\n");
}

#[test]
fn unknown_argument_prints_usage_and_exits_2() {
    Command::cargo_bin("reef")
        .unwrap()
        .arg("--nope")
        .assert()
        .code(2);
}

#[test]
fn no_args_runs_each_line_of_piped_stdin() {
    Command::cargo_bin("reef")
        .unwrap()
        .write_stdin("echo one\necho -n two\n")
        .assert()
        .success()
        .stdout("one\ntwo");
}

#[test]
fn piped_stdin_exits_with_last_status() {
    Command::cargo_bin("reef")
        .unwrap()
        .write_stdin("echo hi\nnosuchcmd_reef_test\n")
        .assert()
        .code(127);
}

#[test]
fn dash_c_without_a_command_is_a_usage_error() {
    Command::cargo_bin("reef")
        .unwrap()
        .arg("-c")
        .assert()
        .code(2);
}

#[test]
fn piped_stdin_handles_crlf_and_a_missing_last_newline() {
    Command::cargo_bin("reef")
        .unwrap()
        .write_stdin("echo a\r\necho b")
        .assert()
        .success()
        .stdout("a\nb\n");
}

/// A program run by one line reads the lines after it, like in bash:
/// the child Reef runs `exit 3`, then the parent Reef runs `echo parent`.
#[test]
fn programs_run_from_piped_stdin_can_read_the_next_lines() {
    let reef = assert_cmd::cargo::cargo_bin("reef");
    Command::cargo_bin("reef")
        .unwrap()
        .write_stdin(format!("'{}'\nexit 3\necho parent\n", reef.display()))
        .assert()
        .success()
        .stdout("parent\n");
}

#[cfg(unix)]
#[test]
fn non_utf8_environment_does_not_crash() {
    use std::os::unix::ffi::OsStrExt;
    Command::cargo_bin("reef")
        .unwrap()
        .env("REEF_BAD", std::ffi::OsStr::from_bytes(b"\xff"))
        .args(["-c", "echo ok"])
        .assert()
        .success()
        .stdout("ok\n");
}

#[test]
fn exit_stops_reading_lines() {
    Command::cargo_bin("reef")
        .unwrap()
        .write_stdin("exit 3\necho after\n")
        .assert()
        .code(3)
        .stdout("");
}

#[test]
fn exit_without_args_uses_last_status() {
    Command::cargo_bin("reef")
        .unwrap()
        .write_stdin("false\nexit\necho after\n")
        .assert()
        .code(1)
        .stdout("");
}

#[test]
fn exit_with_too_many_args_keeps_going() {
    Command::cargo_bin("reef")
        .unwrap()
        .write_stdin("exit 1 2\necho still here\n")
        .assert()
        .success()
        .stdout("still here\n");
}

// ---- cd / pwd: run from the repo root and compare real paths ----

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Run `lines` through piped stdin from the repo root; returns the last line printed.
fn last_line_in_root(lines: &str, envs: &[(&str, &str)]) -> String {
    let o = Command::cargo_bin("reef")
        .unwrap()
        .current_dir(ROOT)
        .envs(envs.iter().copied())
        .write_stdin(lines)
        .output()
        .unwrap();
    assert!(o.status.success(), "{lines:?} failed: {o:?}");
    let out = String::from_utf8(o.stdout).unwrap();
    let last = out.lines().last().unwrap_or_default().to_string();
    // Windows' \\?\ prefix must never reach the user
    assert!(!last.starts_with(r"\\?\"), "{last}");
    last
}

fn same_dir(got: &str, want: &std::path::Path) -> bool {
    std::fs::canonicalize(got).unwrap() == std::fs::canonicalize(want).unwrap()
}

#[test]
fn pwd_prints_starting_dir() {
    let got = last_line_in_root("pwd\n", &[]);
    assert!(same_dir(&got, ROOT.as_ref()), "{got}");
}

#[test]
fn cd_changes_dir_for_later_commands() {
    let got = last_line_in_root("cd src\npwd\n", &[]);
    assert!(
        same_dir(&got, &std::path::Path::new(ROOT).join("src")),
        "{got}"
    );
}

#[test]
fn cd_dotdot_goes_up() {
    let got = last_line_in_root("cd src\ncd ..\npwd\n", &[]);
    assert!(same_dir(&got, ROOT.as_ref()), "{got}");
}

#[test]
fn cd_dash_goes_back_and_prints_it() {
    let got = last_line_in_root("cd src\ncd -\n", &[]);
    assert!(same_dir(&got, ROOT.as_ref()), "{got}");
}

#[test]
fn cd_without_args_goes_home() {
    let home = std::path::Path::new(ROOT).join("src");
    let got = last_line_in_root("cd\npwd\n", &[("HOME", home.to_str().unwrap())]);
    assert!(same_dir(&got, &home), "{got}");
}

#[test]
fn failed_cd_leaves_dir_unchanged() {
    let got = last_line_in_root("cd nosuchdir_reef_test\npwd\n", &[]);
    assert!(same_dir(&got, ROOT.as_ref()), "{got}");
}
