//! Exit statuses with a fixed meaning, the same ones bash uses. Plain 0 and 1
//! (success, failure) stay as numbers.

/// A syntax error, something not supported yet, or bad usage of a builtin or of `reef`.
pub const SYNTAX_ERROR: i32 = 2;
/// The command was found but couldn't start: no permission, a directory, a bad `#!` line.
pub const CANNOT_RUN: i32 = 126;
/// No builtin or program by that name.
pub const NOT_FOUND: i32 = 127;
/// Stopped by Ctrl-C (128 + SIGINT).
pub const INTERRUPTED: i32 = 130;

/// `!`: success becomes failure (1), any failure becomes success (0).
pub fn negate(status: i32) -> i32 {
    if status == 0 { 1 } else { 0 }
}
