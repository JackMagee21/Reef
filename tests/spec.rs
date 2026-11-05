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
    ("exit ' 5 '", "", 5),
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
    // type: builtins win over programs; any name not found makes the status 1
    ("type echo", "echo is a shell builtin\n", 0),
    ("type type", "type is a shell builtin\n", 0),
    (
        "type echo true",
        "echo is a shell builtin\ntrue is a shell builtin\n",
        0,
    ),
    ("type", "", 0),
    ("type nosuch_reef_test", "", 1),
    (
        "type echo nosuch_reef_test cd",
        "echo is a shell builtin\ncd is a shell builtin\n",
        1,
    ),
    ("type nosuch_reef_test || echo missing", "missing\n", 0),
    ("type echo > /dev/null", "", 0),
    ("unset FOO && echo ok", "ok\n", 0),
    ("unset FOO", "", 0),
    ("unset PATH; type echo", "echo is a shell builtin\n", 0),
    ("unset -v FOO && echo ok", "ok\n", 0),
    ("unset a-b || echo failed", "failed\n", 0),
    // export: a bad name fails the whole command, but the good ones are still exported
    ("export REEF_A=1 && echo ok", "ok\n", 0),
    ("export 1bad=x || echo failed", "failed\n", 0),
    ("export =x || echo failed", "failed\n", 0),
    ("export REEF_A", "", 0),
    ("export REEF_A=x=y; export -p > /dev/null", "", 0),
    // variables: $x, ${x}, $? and name=value
    ("x=hello; echo $x", "hello\n", 0),
    ("x=hello; echo ${x}world \"$x!\"", "helloworld hello!\n", 0),
    ("echo $reef_unset_var end", "end\n", 0),
    ("echo \"$reef_unset_var\" end", " end\n", 0),
    ("x='a   b'; echo $x", "a b\n", 0),
    ("x='a   b'; echo \"$x\"", "a   b\n", 0),
    ("echo '$x' \\$x \"\\$x\" $ a$", "$x $x $x $ a$\n", 0),
    ("false; echo $?; echo $?", "1\n0\n", 0),
    ("false || echo $?", "1\n", 0),
    ("x=1 y=$x; echo $y", "1\n", 0),
    ("x=; echo \"[$x]\"", "[]\n", 0),
    ("x=a\"b c\"; echo $x", "ab c\n", 0),
    ("'x=1'", "", 127),
    ("x=1; (x=2; echo $x); echo $x", "2\n1\n", 0),
    ("x=1; { x=2; }; echo $x", "2\n", 0),
    ("x=2; exit $x", "", 2),
    ("x=1; unset x; echo \"[$x]\"", "[]\n", 0),
    // `export x=$y` doesn't split `$y`, like a plain assignment
    ("y='a  b'; export x=$y; echo \"$x\"", "a  b\n", 0),
    (
        "x=reef_echo_arg; echo $x$x",
        "reef_echo_argreef_echo_arg\n",
        0,
    ),
    // pipes: the last command's status counts; `!` flips it
    ("echo a | echo b", "b\n", 0),
    ("false | true", "", 0),
    ("true | false", "", 1),
    ("! true | false", "", 0),
    ("{ echo a; echo b; } | echo c", "c\n", 0),
    ("echo a |\n echo b", "b\n", 0),
    // every command in a pipeline runs in a copy of the shell, like `( )`
    ("x=1; echo a | x=2; echo $x", "1\n", 0),
    ("exit 3 | echo after", "after\n", 0),
    ("echo a | exit 4; echo $?", "4\n", 0),
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
    ("type echo", "", 0),
    (
        "type nosuch_reef_test",
        "reef: type: nosuch_reef_test: not found\n",
        1,
    ),
    // every missing name is reported, not just the first
    (
        "type nosuch_a echo nosuch_b",
        "reef: type: nosuch_a: not found\nreef: type: nosuch_b: not found\n",
        1,
    ),
    // the error goes through Reef's streams, so `2>` can redirect it
    ("type nosuch_reef_test 2>/dev/null", "", 1),
    // Parsed, but running them needs new Io streams (change these when they land)
    (
        "cat <<EOF",
        "reef: not supported yet: heredocs (`<<EOF`)\n",
        2,
    ),
    (
        "echo hi 3>x",
        "reef: not supported yet: file descriptors above 2 (`3>x`)\n",
        2,
    ),
    (
        "echo hi >&-",
        "reef: not supported yet: closing file descriptors (`>&-`)\n",
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
        "reef: not supported yet: background jobs (`&`)\n",
        2,
    ),
    (
        "if true; then echo; fi",
        "reef: not supported yet: `if` (column 1)\n",
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
    (
        "export 1bad=x REEF_OK=1 a-b",
        "reef: export: `1bad=x': not a valid identifier\n\
         reef: export: `a-b': not a valid identifier\n",
        1,
    ),
    ("unset -f", "reef: unset: `-f': not a valid identifier\n", 1),
    ("echo ${x:-y}", "reef: not supported yet: `${x:-y}`\n", 2),
    ("echo hi > ${}", "reef: not supported yet: `${}`\n", 2),
    (
        "echo ${x",
        "reef: syntax error: unexpected end of input: unclosed ${\n",
        2,
    ),
    (
        "x=1 echo hi",
        "reef: not supported yet: assignments before a builtin (`x=... echo`)\n",
        2,
    ),
    (
        "x=reef_nosuch; $x",
        "reef: reef_nosuch: command not found\n",
        127,
    ),
    // Too big for a file descriptor: refused, not run as `echo 99999999999 > x`
    (
        "echo 99999999999>x",
        "reef: not supported yet: file descriptors above 2 (`4294967295>x`)\n",
        2,
    ),
    // An error stops the whole -c line before anything runs
    (
        "echo first; echo a &",
        "reef: not supported yet: background jobs (`&`)\n",
        2,
    ),
    // pipes: a builtin writing into a pipe nobody reads stops quietly
    ("echo a | true", "", 0),
    (
        "echo é | nosuch_reef_cmd",
        "reef: nosuch_reef_cmd: command not found\n",
        127,
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
    let dir = temp_dir("reef_path_test");
    hello_script(&dir, "reef_hello");
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
fn version_flag_prints_the_version() {
    reef()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("reef {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn update_refuses_a_reef_that_cargo_install_didnt_put_there() {
    // The test binary lives in target/debug, not a cargo install `bin` folder
    let out = reef().arg("--update").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("wasn't installed with `cargo install`"),
        "{stderr}"
    );
}

#[test]
fn variables_carry_over_between_piped_lines() {
    piped("x=piped\necho \"$x ${x}\"\n")
        .success()
        .stdout("piped piped\n");
}

#[test]
fn redirect_targets_expand_without_splitting() {
    let dir = temp_dir("reef_redirect_var");
    reef_in(&dir, "f='out file'; echo hi > $f; echo $? >> \"$f\"").success();
    assert_eq!(read(&dir, "out file"), "hi\n0\n");
}

#[cfg(unix)]
#[test]
fn assignments_before_a_program_only_reach_that_program() {
    reef()
        .args([
            "-c",
            "REEF_G=hi sh -c 'echo $REEF_G'; echo \"[$REEF_G]\"; \
             x=local; sh -c 'echo [$x]'; export x; sh -c 'echo [$x]'",
        ])
        .assert()
        .success()
        .stdout("hi\n[]\n[]\n[local]\n");
}

/// A folder with `a.txt b.txt c.rs .hidden sub/d.txt`, for glob tests.
fn glob_dir(name: &str) -> PathBuf {
    let dir = temp_dir(name);
    for file in ["a.txt", "b.txt", "c.rs", ".hidden", "sub/d.txt"] {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }
    dir
}

#[test]
fn globs_match_file_names() {
    let dir = glob_dir("reef_globs");
    let mut failed = vec![];
    for (src, want) in [
        ("echo *.txt", "a.txt b.txt"),
        ("echo *", "a.txt b.txt c.rs sub"),
        ("echo ?.rs", "c.rs"),
        ("echo [ab].txt", "a.txt b.txt"),
        ("echo [!a].txt", "b.txt"),
        ("echo */*.txt", "sub/d.txt"),
        ("echo s*/", "sub/"),
        ("echo .*", ".hidden"),
        ("echo sub/*", "sub/d.txt"),
        // no match: the word is kept as typed, like sh
        ("echo *.none", "*.none"),
        ("echo nosuch/*", "nosuch/*"),
        // quoted or escaped, a `*` is just a character
        ("echo '*.txt' \"*.txt\" \\*.txt", "*.txt *.txt *.txt"),
        ("echo \"a\"*.txt", "a.txt"),
        ("echo [", "["),
        // an unquoted `$x` is globbed, a quoted one isn't
        ("x='*.txt'; echo $x", "a.txt b.txt"),
        ("x='*.txt'; echo \"$x\"", "*.txt"),
        // `x=*` doesn't glob; `$x` later does
        ("x=*.rs; echo \"$x\" $x", "*.rs c.rs"),
    ] {
        let out = reef().current_dir(&dir).args(["-c", src]).output().unwrap();
        let got = String::from_utf8_lossy(&out.stdout);
        if got.trim_end() != want {
            failed.push(format!("{src:?}\n  want {want:?}\n  got  {got:?}"));
        }
    }
    assert!(failed.is_empty(), "\n{}", failed.join("\n"));
}

#[test]
fn tilde_is_the_home_directory() {
    let mut failed = vec![];
    for (src, want) in [
        ("echo ~", "/h/me"),
        ("echo ~/x ~/", "/h/me/x /h/me/"),
        ("x=~/bin; echo $x", "/h/me/bin"),
        ("export x=~; echo $x", "/h/me"),
        // only `~` or `~/` at the start of a word, and not quoted
        ("echo '~' \"~\" \\~ a~ ~x ~\"/x\"", "~ ~ ~ a~ ~x ~/x"),
    ] {
        let out = reef()
            .env("HOME", "/h/me")
            .args(["-c", src])
            .output()
            .unwrap();
        let got = String::from_utf8_lossy(&out.stdout);
        if got.trim_end() != want {
            failed.push(format!("{src:?}\n  want {want:?}\n  got  {got:?}"));
        }
    }
    assert!(failed.is_empty(), "\n{}", failed.join("\n"));
}

#[test]
fn a_tilde_home_is_never_split_or_globbed() {
    let dir = glob_dir("reef_tilde_glob");
    reef()
        .current_dir(&dir)
        .env("HOME", "my *  home")
        .args(["-c", "echo ~"])
        .assert()
        .success()
        .stdout("my *  home\n");
}

#[test]
fn cd_tilde_goes_home() {
    let dir = temp_dir("reef_cd_tilde");
    let out = reef()
        .env("HOME", &dir)
        .args(["-c", "cd /; cd ~ && pwd"])
        .output()
        .unwrap();
    let got = String::from_utf8_lossy(&out.stdout);
    assert!(same_dir(got.trim_end(), &dir), "{got}");
}

/// `src` with `REEF` replaced by the quoted path of the reef being tested, so
/// a pipe can feed a second reef: a reader that exists on Windows and Linux.
fn with_reef(src: &str) -> String {
    src.replace("REEF", &format!("'{}'", env!("CARGO_BIN_EXE_reef")))
}

#[test]
fn a_pipe_feeds_one_command_into_the_next() {
    reef()
        .args(["-c", &with_reef("echo 'echo from the pipe' | REEF")])
        .assert()
        .success()
        .stdout("from the pipe\n");
}

#[test]
fn pipes_chain_through_several_commands() {
    reef()
        .args(["-c", &with_reef("echo 'echo echo deep' | REEF | REEF")])
        .assert()
        .success()
        .stdout("deep\n");
}

#[test]
fn a_pipeline_gets_the_status_of_its_last_command() {
    reef()
        .args(["-c", &with_reef("echo 'exit 4' | REEF")])
        .assert()
        .code(4);
}

#[test]
fn redirects_apply_inside_a_pipeline() {
    // `2>&1` before the pipe sends the error down the pipe too
    reef()
        .args([
            "-c",
            &with_reef("nosuch_reef_cmd 2>&1 | REEF -c 'echo got it'"),
        ])
        .assert()
        .success()
        .stdout("got it\n");
    let dir = temp_dir("reef_pipe_redirect");
    reef_in(&dir, &with_reef("echo 'echo hi' | REEF > out")).success();
    assert_eq!(read(&dir, "out"), "hi\n");
}

#[test]
fn pipes_work_from_piped_stdin_too() {
    piped(with_reef("echo 'echo inner' | REEF\necho after\n"))
        .success()
        .stdout("inner\nafter\n");
}

#[cfg(unix)]
#[test]
fn unix_pipes_with_real_tools() {
    for (src, want) in [
        ("printf 'a\\nb\\n' | grep b", "b\n"),
        ("echo hi | cat | cat", "hi\n"),
        ("echo one two | wc -w", "2\n"),
    ] {
        let out = reef().args(["-c", src]).output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim_start(),
            want,
            "{src}"
        );
    }
}

/// The classic pipe bug: if Reef keeps a copy of a pipe end open, `yes` never
/// learns that `head` is gone, and this never finishes.
#[cfg(unix)]
#[test]
fn a_pipeline_ends_when_its_reader_stops() {
    reef()
        .args(["-c", "yes | head -n 2"])
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .success()
        .stdout("y\ny\n");
}

#[test]
fn invalid_utf8_on_piped_stdin_does_not_stop_the_script() {
    piped(&b"echo \xff\necho after\n"[..])
        .success()
        .stdout("\u{FFFD}\nafter\n");
}

#[test]
fn export_lists_exported_variables_sorted_and_quoted() {
    let out = reef()
        .env("REEF_Z", "z")
        .env("REEF_Q", "it's")
        .args(["-c", "export REEF_A='a b'; export"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let mine: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("export REEF_"))
        .collect();
    assert_eq!(
        mine,
        [
            "export REEF_A='a b'",
            r"export REEF_Q='it'\''s'",
            "export REEF_Z='z'"
        ]
    );
}

#[cfg(unix)]
#[test]
fn export_reaches_child_processes_and_bad_names_do_not() {
    let out = reef()
        .args(["-c", "export REEF_X=hi =bad 1bad=y; env"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.lines().any(|l| l == "REEF_X=hi"), "{stdout}");
    assert!(
        !stdout
            .lines()
            .any(|l| l.starts_with('=') || l.starts_with("1bad=")),
        "{stdout}"
    );
}

/// An executable script `name` in `dir` that prints `hello`.
#[cfg(unix)]
fn hello_script(dir: &Path, name: &str) {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join(name);
    std::fs::write(&script, "#!/bin/sh\necho hello\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn unset_path_never_searches_the_current_dir() {
    let dir = temp_dir("reef_unset_path");
    hello_script(&dir, "reef_hello");
    reef_in(&dir, "unset PATH; reef_hello")
        .code(127)
        .stdout("")
        .stderr("reef: reef_hello: command not found\n");
}

#[cfg(unix)]
#[test]
fn cd_into_a_dir_without_search_permission_fails() {
    use std::os::unix::fs::PermissionsExt;
    // root can enter anything, so there's nothing to check
    if is_root() {
        return;
    }
    let dir = temp_dir("reef_cd_locked");
    let locked = dir.join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let result = reef_in(&dir, "cd locked; pwd").get_output().clone();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&result.stderr),
        "reef: cd: locked: Permission denied\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout).trim_end(),
        dir.canonicalize().unwrap().display().to_string()
    );
}

#[cfg(unix)]
fn is_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|o| o.stdout == b"0\n")
}

#[cfg(unix)]
#[test]
fn a_deleted_current_dir_is_not_command_not_found() {
    let dir = temp_dir("reef_deleted_cwd");
    hello_script(&dir, "reef_hello");
    std::fs::create_dir(dir.join("sub")).unwrap();
    let dir = dir.canonicalize().unwrap();
    let script = dir.join("reef_hello");
    reef_in(
        &dir,
        &format!("cd sub && rmdir ../sub && {}", script.display()),
    )
    .code(126)
    .stderr(format!(
        "reef: {}: current directory {} no longer exists\n",
        script.display(),
        dir.join("sub").display()
    ));
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

// ---- type: finding real programs (paths differ per machine, so not in the tables) ----

#[test]
fn type_shows_the_path_of_a_program_given_by_path() {
    let exe = assert_cmd::cargo::cargo_bin("reef");
    let src = format!("type '{}'", exe.display());
    let out = reef().args(["-c", &src]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.contains(&format!("is {}", exe.display())),
        "{stdout}"
    );
}

#[test]
fn type_finds_programs_on_path() {
    // PATH holds only the folder with Reef's binary, so `reef` must be found there
    // (on Windows `reef` becomes `reef.exe` through PATHEXT)
    let exe = assert_cmd::cargo::cargo_bin("reef");
    let dir = exe.parent().unwrap();
    let out = reef()
        .env("PATH", dir)
        .args(["-c", "type reef"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(stdout.starts_with("reef is "), "{stdout}");
    assert!(stdout.contains(&dir.display().to_string()), "{stdout}");
}

#[test]
fn type_prefers_a_builtin_over_a_program_with_the_same_name() {
    // a fake `echo` program on PATH must not win over the builtin
    let dir = temp_dir("type_builtin_wins");
    let exe = assert_cmd::cargo::cargo_bin("reef");
    let fake = dir.join(
        exe.file_name()
            .unwrap()
            .to_string_lossy()
            .replace("reef", "echo"),
    );
    std::fs::copy(&exe, &fake).unwrap();
    reef()
        .env("PATH", &dir)
        .args(["-c", "type echo"])
        .assert()
        .success()
        .stdout("echo is a shell builtin\n");
}
