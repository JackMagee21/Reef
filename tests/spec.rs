use assert_cmd::Command;

// (input, expected stdout, expected exit code)
// Add every new feature's cases here BEFORE implementing it, and never delete a passing case.
// Only use inputs that behave the same on Windows and Linux (Reef's own builtins, not ls/dir).
const CASES: &[(&str, &str, i32)] = &[
    ("", "", 0),
    ("   ", "", 0),
    ("nosuchcmd_reef_test", "", 127),
    ("echo hi", "hi\n", 0),
    ("echo a'b'c", "abc\n", 0),
    ("echo -n", "", 0),
    ("echo -n abc", "abc", 0)
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
fn no_args_prints_usage_and_exits_2() {
    Command::cargo_bin("reef").unwrap().assert().code(2);
}
