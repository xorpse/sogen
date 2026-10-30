//! Build script support for binaries using `sogen`.
//!
//! Sogen loads some backends from shared libraries (FEXCore on ARM64 hosts, every backend with the `shared` feature).
//! Call [`configure_linkage`] from the build script of a package with binaries, tests or examples that use `sogen`.

use std::env;
use std::path::PathBuf;

/// Set by `sogen-native`'s build script for its direct dependents.
const NATIVE_RUNTIME_DIRS: &str = "DEP_SOGEN_NATIVE_RUNTIME_DIRS";
/// Set by `sogen`'s build script, forwarding the value from `sogen-native`.
const RUNTIME_DIRS: &str = "DEP_SOGEN_RUNTIME_DIRS";

/// The directories of the shared libraries Sogen loads at run time.
///
/// # Panics
///
/// When not called from the build script of a package depending directly on `sogen`.
pub fn runtime_dirs() -> Vec<PathBuf> {
    let dirs = env::var_os(NATIVE_RUNTIME_DIRS)
        .or_else(|| env::var_os(RUNTIME_DIRS))
        .expect("`sogen_build` requires a direct dependency on `sogen`");
    env::split_paths(&dirs)
        .filter(|dir| !dir.as_os_str().is_empty())
        .collect()
}

/// Puts [`runtime_dirs`] on the rpath of the calling package's binaries, tests and examples. Windows has no rpath, so
/// there the directories have to be on `PATH` when running outside of Cargo.
pub fn configure_linkage() {
    if env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        return;
    }

    for dir in runtime_dirs() {
        println!("cargo::rustc-link-arg=-Wl,-rpath,{}", dir.display());
    }
}
