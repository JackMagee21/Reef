use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use assert_cmd::assert::Assert;

/// A fresh `reef` command to configure and run.
fn reef() -> Command {
    Command::cargo_bin("reef").unwrap()
}

/// Run `reef` with no arguments, feeding it `input` as piped stdin.
fn piped(input: impl Into<Vec<u8>>) -> Assert {
    reef().write_stdin(input).assert()
}

/// A fresh, empty folder under `target/` for one test, so tests never write into the repo.
fn temp_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `reef -c src`, run inside `dir`.
fn reef_in(dir: &Path, src: &str) -> Assert {
    reef().current_dir(dir).args(["-c", src]).assert()
}

fn read(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap()
}

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
    // Quoted or escaped, operator characters are just text
    ("echo 'a;b' \"c|d\" e\\&f g\\>h", "a;b c|d e&f g>h\n", 0),
    ("echo hi\n", "hi\n", 0),
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
    // Lists: `;` and newlines run commands in order
    ("echo a; echo b", "a\nb\n", 0),
    ("echo a;echo b", "a\nb\n", 0),
    ("echo a\necho b", "a\nb\n", 0),
    ("echo a;", "a\n", 0),
    ("false; true", "", 0),
    ("true; false", "", 1),
    // && and ||: the right side runs only if the left succeeded / failed
    ("true && echo yes", "yes\n", 0),
    ("false && echo no", "", 1),
    ("false || echo yes", "yes\n", 0),
    ("true || echo no", "", 0),
    ("true && false || echo c", "c\n", 0),
    ("false || false && echo no", "", 1),
    ("echo a &&\necho b", "a\nb\n", 0),
    // ! flips the status
    ("! true", "", 1),
    ("! false", "", 0),
    ("! false && echo yes", "yes\n", 0),
    // { } runs in this shell, ( ) in a copy
    ("{ echo a; echo b; }", "a\nb\n", 0),
    ("{ echo a; } && echo b", "a\nb\n", 0),
    ("{ false; }", "", 1),
    ("(echo sub)", "sub\n", 0),
    ("(exit 3)", "", 3),
    ("(exit 3); echo after", "after\n", 0),
    ("(exit 3) || echo failed", "failed\n", 0),
    // exit stops the rest of the line, and uses $? from the command before it
    ("exit 4; echo no", "", 4),
    ("false; exit", "", 1),
    ("true && exit 5 || echo no", "", 5),
    // Reserved words are only special at the start of a command
    ("echo }", "}\n", 0),
    ("echo { if !", "{ if !\n", 0),
    ("echo '{'", "{\n", 0),
    // Redirects that don't create files (the rest are tests below, in a temp folder)
    ("echo hi > /dev/null", "", 0),
    ("echo hi 1>&2", "", 0),
    (
        "nosuchcmd_reef_test 2>&1",
        "reef: nosuchcmd_reef_test: command not found\n",
        127,
    ),
    // b goes to the group's stderr, which is /dev/null
    ("{ echo a; echo b 1>&2; } 2>/dev/null", "a\n", 0),
    ("{ echo a; echo b; } >/dev/null; echo c", "c\n", 0),
    ("echo hi < missing_reef_file || echo failed", "failed\n", 0),
];

#[test]
fn spec() {
    check_table(CASES, |o| &o.stdout);
}

/// Run every row as `reef -c input` and compare one output stream and the exit code.
/// Collects all failures before failing, so one run shows everything that's wrong.
fn check_table(rows: &[(&str, &str, i32)], stream: fn(&Output) -> &[u8]) {
    let mut failed = vec![];
    for (src, want, code) in rows {
        let o = reef().args(["-c", src]).output().unwrap();
        let got = String::from_utf8_lossy(stream(&o));
        let got_code = o.status.code().unwrap_or(-1);
        if (got.as_ref(), got_code) != (*want, *code) {
            failed.push(format!(
                "{src:?}\n  want {want:?} {code}\n  got  {got:?} {got_code}"
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
    // Parsed, but running them needs new Io streams (change these when they land)
    (
        "echo é | x",
        "reef: syntax error: pipelines (`|`) are not supported yet\n",
        2,
    ),
    (
        "cat <<EOF",
        "reef: syntax error: heredocs (`<<EOF`) are not supported yet\n",
        2,
    ),
    (
        "echo hi 3>x",
        "reef: syntax error: file descriptors above 2 (`3>x`) are not supported yet\n",
        2,
    ),
    (
        "echo hi >&-",
        "reef: syntax error: closing file descriptors (`>&-`) is not supported yet\n",
        2,
    ),
    // Redirect errors: the command doesn't run, status 1
    (
        "echo hi < missing_reef_file",
        "reef: missing_reef_file: No such file or directory\n",
        1,
    ),
    ("echo hi 2>&9", "reef: 9: Bad file descriptor\n", 1),
    ("echo hi >&x", "reef: x: Bad file descriptor\n", 1),
    // Redirects that don't create files (the rest are tests below, in a temp folder)
    ("echo hi 1>&2", "hi\n", 0),
    ("echo hi >&2", "hi\n", 0),
    ("nosuchcmd_reef_test 2>/dev/null", "", 127),
    (
        "true &",
        "reef: syntax error: background jobs (`&`) are not supported yet\n",
        2,
    ),
    (
        "if true; then echo; fi",
        "reef: syntax error: `if` is not supported yet (column 1)\n",
        2,
    ),
    // Syntax errors point at the column
    (
        "echo a;;",
        "reef: syntax error: unexpected `;;` (column 7)\n",
        2,
    ),
    (
        "; echo a",
        "reef: syntax error: unexpected `;` (column 1)\n",
        2,
    ),
    (
        "echo é )",
        "reef: syntax error: unexpected `)` (column 8)\n",
        2,
    ),
    ("}", "reef: syntax error: unexpected `}` (column 1)\n", 2),
    ("( )", "reef: syntax error: unexpected `)` (column 3)\n", 2),
    (
        "echo >",
        "reef: syntax error: `>` needs a file name after it (column 6)\n",
        2,
    ),
    (
        "echo > ; x",
        "reef: syntax error: `>` needs a file name after it (column 6)\n",
        2,
    ),
    // Unfinished input: an error with -c, more lines in piped mode and at the prompt
    (
        "echo a &&",
        "reef: syntax error: unexpected end of input: `&&` needs a command after it\n",
        2,
    ),
    (
        "echo a |",
        "reef: syntax error: unexpected end of input: `|` needs a command after it\n",
        2,
    ),
    (
        "{ echo a;",
        "reef: syntax error: unexpected end of input: `{` is missing its `}`\n",
        2,
    ),
    (
        "(echo a",
        "reef: syntax error: unexpected end of input: `(` is missing its `)`\n",
        2,
    ),
    // An error stops the whole -c line before anything runs
    (
        "echo first; echo a | b",
        "reef: syntax error: pipelines (`|`) are not supported yet\n",
        2,
    ),
];

#[test]
fn error_messages() {
    check_table(ERRORS, |o| &o.stderr);
}

#[test]
fn piped_quotes_can_span_lines() {
    piped("echo 'a\nb'\necho next\n")
        .success()
        .stdout("a\nb\nnext\n");
}

#[test]
fn piped_commands_can_continue_after_an_operator() {
    piped("echo a &&\necho b\n{\necho c\n}\n")
        .success()
        .stdout("a\nb\nc\n");
}

#[test]
fn piped_blank_lines_keep_the_last_status() {
    piped("false\n\n# just a comment\n").code(1);
}

#[test]
fn piped_backslash_newline_joins_lines() {
    piped("echo a \\\nb\n").success().stdout("a b\n");
}

/// POSIX: a syntax error stops a script, it doesn't skip to the next line.
#[test]
fn piped_syntax_error_stops_the_script() {
    piped("echo one\necho 'never closed\necho three\n")
        .code(2)
        .stdout("one\n");
}

#[cfg(unix)]
#[test]
fn a_file_that_cant_run_is_126_not_127() {
    // Cargo.toml exists but has no execute bit
    reef()
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
    reef()
        .env("PATH", ":/usr/bin:/bin")
        .write_stdin(format!("cd '{}'\nreef_hello\n", dir.display()))
        .assert()
        .success()
        .stdout("hello\n");
}

#[test]
fn unknown_argument_prints_usage_and_exits_2() {
    reef().arg("--nope").assert().code(2);
}

#[test]
fn no_args_runs_each_line_of_piped_stdin() {
    piped("echo one\necho -n two\n")
        .success()
        .stdout("one\ntwo");
}

#[test]
fn piped_stdin_exits_with_last_status() {
    piped("echo hi\nnosuchcmd_reef_test\n").code(127);
}

#[test]
fn dash_c_without_a_command_is_a_usage_error() {
    reef().arg("-c").assert().code(2);
}

#[test]
fn piped_stdin_handles_crlf_and_a_missing_last_newline() {
    piped("echo a\r\necho b").success().stdout("a\nb\n");
}

/// A program run by one line reads the lines after it, like in bash:
/// the child Reef runs `exit 3`, then the parent Reef runs `echo parent`.
#[test]
fn programs_run_from_piped_stdin_can_read_the_next_lines() {
    let child = assert_cmd::cargo::cargo_bin("reef");
    piped(format!("'{}'\nexit 3\necho parent\n", child.display()))
        .success()
        .stdout("parent\n");
}

#[cfg(unix)]
#[test]
fn non_utf8_environment_does_not_crash() {
    use std::os::unix::ffi::OsStrExt;
    reef()
        .env("REEF_BAD", std::ffi::OsStr::from_bytes(b"\xff"))
        .args(["-c", "echo ok"])
        .assert()
        .success()
        .stdout("ok\n");
}

#[test]
fn exit_stops_reading_lines() {
    piped("exit 3\necho after\n").code(3).stdout("");
}

#[test]
fn exit_without_args_uses_last_status() {
    piped("false\nexit\necho after\n").code(1).stdout("");
}

#[test]
fn exit_with_too_many_args_keeps_going() {
    piped("exit 1 2\necho still here\n")
        .success()
        .stdout("still here\n");
}

// ---- cd / pwd: run from the repo root and compare real paths ----

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Run `lines` through piped stdin from the repo root; returns the last line printed.
fn last_line_in_root(lines: &str, envs: &[(&str, &str)]) -> String {
    let o = reef()
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

// ---- redirects that create files: each test gets its own temp folder ----

#[test]
fn redirects_write_append_and_truncate_files() {
    let dir = temp_dir("redirect_files");
    for (src, file, want) in [
        ("echo hi > out", "out", "hi\n"),
        ("echo a > trunc; echo b > trunc", "trunc", "b\n"),
        ("echo a > log; echo b >> log", "log", "a\nb\n"),
        ("> first echo x", "first", "x\n"),
        ("echo hi >| clobber", "clobber", "hi\n"),
        ("echo hi 1> one", "one", "hi\n"),
        // `<>` opens for reading and writing without truncating; `1` points stdout at it
        ("echo hi 1<> rw", "rw", "hi\n"),
        ("{ echo a; echo b; } > group", "group", "a\nb\n"),
        ("(echo sub) > sub", "sub", "sub\n"),
        ("echo a > x && echo b >> x", "x", "a\nb\n"),
    ] {
        reef_in(&dir, src).success().stdout("");
        assert_eq!(read(&dir, file), want, "{src:?}");
    }
}

#[test]
fn redirect_order_matters() {
    let dir = temp_dir("redirect_order");
    let message = "reef: nosuchcmd_reef_test: command not found\n";
    // `2> err`: only stderr goes to the file
    reef_in(&dir, "nosuchcmd_reef_test 2> err")
        .code(127)
        .stdout("")
        .stderr("");
    assert_eq!(read(&dir, "err"), message);
    // `> both 2>&1`: stdout moves to the file, then stderr copies it there too
    reef_in(&dir, "nosuchcmd_reef_test > both 2>&1")
        .code(127)
        .stdout("")
        .stderr("");
    assert_eq!(read(&dir, "both"), message);
    // `2>&1 > out`: stderr copies stdout while it's still the screen, then stdout moves
    reef_in(&dir, "nosuchcmd_reef_test 2>&1 > out")
        .code(127)
        .stdout(message)
        .stderr("");
    assert_eq!(read(&dir, "out"), "");
}

#[test]
fn redirect_paths_are_relative_to_the_shells_directory() {
    let dir = temp_dir("redirect_cd");
    std::fs::create_dir(dir.join("sub")).unwrap();
    reef_in(&dir, "cd sub && echo hi > f").success();
    assert_eq!(read(&dir, "sub/f"), "hi\n");
}

#[test]
fn programs_get_redirected_files_as_stdin_and_stdout() {
    let dir = temp_dir("redirect_programs");
    std::fs::write(dir.join("script"), "echo from-script\n").unwrap();
    let child = assert_cmd::cargo::cargo_bin("reef");
    // the child Reef reads its commands from the file and writes to another file
    reef_in(&dir, &format!("'{}' < script > out", child.display()))
        .success()
        .stdout("");
    assert_eq!(read(&dir, "out"), "from-script\n");
}

#[test]
fn a_failed_redirect_skips_the_command_but_not_the_script() {
    let dir = temp_dir("redirect_fail");
    reef()
        .current_dir(&dir)
        .write_stdin("echo a > /nonexistent_reef_dir/x\necho b\n")
        .assert()
        .success()
        .stdout("b\n");
    // a directory can't be written as a file (the message differs by OS)
    reef_in(&dir, "echo hi > .").code(1).stdout("");
}
