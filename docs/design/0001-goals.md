# 0001: Goals

Reef is the successor to RShell, rebuilt from the ground up.

## Decisions

1. **Native Windows first.** Linux must pass CI on every push.
2. **A documented POSIX sh subset.** What's missing is written down, not discovered.
3. **`\` is always the POSIX escape.** Paths use `/` or quotes (`'C:\Users'`). Tab completion inserts `/`.
4. **All OS-specific code lives in `src/platform/`.** Nothing else uses `#[cfg(windows)]` or `#[cfg(unix)]`. `tests/layout.rs` checks this.
5. **The shell owns its current directory** (`Shell::cwd`). Reef never calls `set_current_dir`, because builtins in pipelines run on threads that would share it.

## Lessons from RShell

Taken from RShell's `docs/rebuild/01-DOS-AND-DONTS.md`, where each rule comes with what it cost last time.

1. **Quoting information must survive past the lexer.** Never turn input into plain `String`s early on.
2. **Parse in exactly one place.** The REPL, rc file, `source`, function bodies and `$(...)` all call the same parser.
3. **Built-ins write to a given output stream.** Never call `println!` inside a built-in.
4. **Write tests first, starting from day one.** No feature counts as done without a test.
5. **Every fact is stored in one place.** One list of built-ins, one history writer, one temp-file function.
6. **Never slice strings by byte position by hand.** Use `char_indices()`, `.get()`, or a lexer that walks characters.
7. **Never throw errors away silently.** No `let _ =`, `.ok()` or `unwrap_or(0)` on anything the user would care about.
8. **Build and test on Windows *and* Linux**, using CI on both.
9. **The README only describes what has tests.** Mark everything else "planned".
10. **Go deep, not wide.** Finish the core before adding commands, a package manager, an editor or a GUI.
