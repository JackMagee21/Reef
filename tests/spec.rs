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
