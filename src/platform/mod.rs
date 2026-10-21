//! The only place in Reef that knows which OS it's on.
//!
//! Each OS file defines the same functions with the same signatures, and the
//! compiler picks one. Code outside `platform/` calls these and never uses
//! `#[cfg(windows)]` or `#[cfg(unix)]` itself (CI checks this).

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;
