# The platform layer

`src/platform/` is the only part of Reef that knows which operating system it's running on. Everything above it is the same on Windows and Linux.

```
Reef logic (lexer, builtins, run, repl...)    no #[cfg] anywhere
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
| `find_command(name, path, cwd)` | tries each `PATHEXT` extension (`.exe`, `.cmd`, `.bat`...); a name typed with its extension is used as-is | the file must have an executable bit |
| | a name containing `/` or `\` is looked up relative to `cwd`, not `PATH` | a name containing `/` is looked up relative to `cwd` |
| `exit_status(status)` | Ctrl-C exit code `0xC000013A` becomes 130 | killed by signal N becomes 128 + N |
| `var_key(name)` | uppercases: `Path` and `PATH` are one variable | unchanged: names are case-sensitive |

**Planned:** console setup (UTF-8, VT mode), Ctrl-C details, `canonicalize` without the `\\?\` prefix, case-insensitive globbing on Windows, home and config directories, and job control (`fg`/`bg` on Linux only).

## The rules

1. **`#[cfg(windows)]` / `#[cfg(unix)]` only appear inside `src/platform/`.** CI runs a `grep` that fails the build if they appear anywhere else. If you need one somewhere else, a function is missing from this layer.
2. **Both files define the same functions with the same signatures.** Add to `windows.rs` and `unix.rs` together.
3. **No shell logic here.** The layer answers OS questions ("where is this program?", "what does this exit code mean?"). Parsing, expansion and builtin behavior never move into it.

## Adding a platform function

1. Write it in `windows.rs` and `unix.rs` with the same name and signature.
2. Call it from Reef code as `platform::name(...)`.
3. Check both builds: `make check` runs clippy for Windows and the Linux target.
