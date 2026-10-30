//! Raw cxx bindings to sogen. Use the `sogen` crate for a safe interface.

#[cfg(feature = "static")]
use icicle as _;

pub mod arm64;
pub mod common;
pub mod linux;
pub mod windows;
