# The platform layer

`src/platform/` is the only part of Reef that knows which operating system it's running on. Everything above it is the same on Windows and Linux.

```
Reef logic (lexer, builtins, run, repl...)    no OS #[cfg] anywhere
        │  calls platform::find_command() etc.
        ▼
platform/mod.rs     picks ONE of these when compiling
   ├── windows.rs   (Windows builds)
   └── unix.rs      (Linux builds)
        │
        ▼
Rust std + the OS
```

## Why the OS is picked at compile time

A Windows `.exe` can only ever run on Windows, so there's nothing to detect at runtime. `#[cfg(windows)]` and `#[cfg(unix)]` in `platform/mod.rs` make the compiler include only the right file:

- Windows-only APIs don't need to compile on Linux, and the reverse.
- There's no runtime check and no cost.
- If one file is missing a function the other has, that platform's build fails, so CI catches it.

Rust's standard library already hides most OS differences (files, processes, env vars). The platform layer only covers the places where Windows and Linux genuinely behave differently, so it stays small.

## What's in it

| Function | Windows | Linux |
|---|---|---|
| `find_command(name, sh)` | tries each extension in the shell's `PATHEXT` (`.exe`, `.cmd`, `.bat`...); a name typed with its extension is used as-is | the file must have an executable bit |
| | a name containing `/` or `\` is looked up relative to `cwd`, not `PATH`. Relative `PATH` entries are relative to `cwd` | a name containing `/` is looked up relative to `cwd` and used if it exists (so a file without the execute bit gives 126, not 127). Empty and relative `PATH` entries mean `cwd` |
| `exit_status(status)` | always 0-255. Crash codes become 128 + the matching Unix signal (Ctrl-C `0xC000013A` → 130, access violation `0xC0000005` → 139); other codes keep their low 8 bits (`300` → 44), and a failure never becomes 0 | killed by signal N becomes 128 + N |
| `var_key(name)` | uppercases: `Path` and `PATH` are one variable | unchanged: names are case-sensitive |
| `HOME_VARS` (a constant) | `HOME`, then `USERPROFILE`: where `cd` with no argument goes | `HOME` |
| `resolve(cwd, target)` | `cwd.join(target)`, except that `C:` / `C:dir` use `cwd` if it's on drive C, else `C:\` (Windows would otherwise use the process's own directory, which Reef never changes) | `cwd.join(target)` |
| `stdin_file()` | Reef's stdin as an unbuffered `File`, for piped mode | same |
| `redirect_path(cwd, target)` | the file `> target` opens: `resolve`, except `/dev/null` and `NUL` become the null device `\\.\NUL` | `resolve` (`/dev/null` already exists) |
| `clean_path(path)` | removes the `\\?\` prefix `canonicalize` adds (`\\?\UNC\server\x` becomes `\\server\x`) | returns the path unchanged |

**Planned:** console setup (UTF-8, VT mode), Ctrl-C details, case-insensitive globbing on Windows, home and config directories, and job control (`fg`/`bg` on Linux only).

## The rules

1. **`#[cfg(windows)]` / `#[cfg(unix)]` only appear inside `src/platform/`.** `tests/layout.rs` fails `cargo test` if they appear anywhere else, including `cfg!(...)`, `cfg_attr(...)` and `target_os` / `target_family` / `target_env` / `target_vendor`. If you need one somewhere else, a function is missing from this layer.
2. **Both files define the same functions with the same signatures.** Add to `windows.rs` and `unix.rs` together.
3. **No shell logic here.** The layer answers OS questions ("where is this program?", "what does this exit code mean?"). Parsing, expansion and builtin behavior never move into it.

## Adding a platform function

1. Write it in `windows.rs` and `unix.rs` with the same name and signature.
2. Call it from Reef code as `platform::name(...)`.
3. Check both builds: `make check` runs clippy for Windows and the Linux target.
