use std::path::{Path, PathBuf};
use std::{env, fs};

// MSVC cannot mix debug and release C++ runtimes, and Rust always links the release one.
const CMAKE_CONFIG: &str = "RelWithDebInfo";

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let source_dir = manifest_dir
        .join("../../..")
        .canonicalize()
        .expect("sogen source tree");
    let sys_dir = source_dir.join("src/rust-bindings/sogen-sys");
    let bridge_dir = out_dir.join("cxxbridge");

    let static_build = env::var_os("CARGO_FEATURE_STATIC").is_some();
    let shared_build = env::var_os("CARGO_FEATURE_SHARED").is_some();
    if static_build == shared_build {
        panic!("sogen-native needs exactly one of the `static` and `shared` features");
    }

    generate_bridges(&sys_dir, &bridge_dir);
    track_sources(&source_dir, &sys_dir);

    let build_dir = build_sogen(&source_dir, &out_dir, &bridge_dir, static_build);
    let link_manifest = build_dir.join(format!("sogen-rust-link-{CMAKE_CONFIG}.txt"));
    let runtime_manifest = build_dir.join(format!("sogen-rust-runtime-{CMAKE_CONFIG}.txt"));
    let runtime = fs::read_to_string(&runtime_manifest).unwrap_or_else(|error| {
        panic!(
            "read runtime manifest {}: {error}",
            runtime_manifest.display()
        )
    });
    let runtime_dirs = env::join_paths(runtime.lines().filter(|dir| !dir.is_empty()))
        .expect("join runtime dirs")
        .into_string()
        .expect("runtime directories are UTF-8");

    println!("cargo::metadata=link_manifest={}", link_manifest.display());
    println!("cargo::metadata=runtime_dirs={runtime_dirs}");
}

fn generate_bridges(sys_dir: &Path, bridge_dir: &Path) {
    let include_dir = bridge_dir.join("include");
    let header_dir = include_dir.join("sogen-sys");
    fs::create_dir_all(include_dir.join("rust")).expect("create cxx include directory");
    fs::create_dir_all(&header_dir).expect("create bridge header directory");
    write_if_changed(&include_dir.join("rust/cxx.h"), cxx_gen::HEADER.as_bytes());

    let mut generated_files = Vec::new();
    for entry in fs::read_dir(sys_dir.join("src")).expect("read sogen-sys sources") {
        let path = entry.expect("source entry").path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }

        let source = fs::read_to_string(&path).expect("read bridge source");
        if !source.contains("#[cxx::bridge") {
            continue;
        }

        let tokens = source
            .parse::<proc_macro2::TokenStream>()
            .expect("tokenize bridge source");
        let generated = cxx_gen::generate_header_and_cc(tokens, &cxx_gen::Opt::default())
            .unwrap_or_else(|error| panic!("cxx bridge {}: {error}", path.display()));

        let name = path
            .file_name()
            .expect("bridge file name")
            .to_string_lossy();
        let stem = path.file_stem().expect("bridge stem").to_string_lossy();
        let header = header_dir.join(format!("{name}.h"));
        let implementation = bridge_dir.join(format!("{stem}.cc"));
        write_if_changed(&header, &generated.header);
        write_if_changed(&implementation, &generated.implementation);
        generated_files.extend([header, implementation]);
    }

    for dir in [bridge_dir, header_dir.as_path()] {
        for entry in fs::read_dir(dir).expect("read generated bridge directory") {
            let path = entry.expect("generated bridge entry").path();
            if path.is_file() && !generated_files.contains(&path) {
                fs::remove_file(&path).expect("remove stale bridge file");
            }
        }
    }
}

// Keeps file timestamps stable so CMake only recompiles bridges whose code actually changed.
fn write_if_changed(path: &Path, content: &[u8]) {
    if fs::read(path).is_ok_and(|existing| existing == content) {
        return;
    }

    fs::write(path, content).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

fn track_sources(source_dir: &Path, sys_dir: &Path) {
    println!("cargo::rerun-if-env-changed=CMAKE_GENERATOR");
    for path in [
        sys_dir.join("src"),
        sys_dir.join("cpp"),
        source_dir.join("CMakeLists.txt"),
        source_dir.join("cmake"),
        source_dir.join("deps"),
    ] {
        println!("cargo::rerun-if-changed={}", path.display());
    }

    // The Cargo target directory lives below src/rust-bindings, so that directory itself must not be tracked.
    let rust_bindings = source_dir.join("src/rust-bindings");
    println!(
        "cargo::rerun-if-changed={}",
        rust_bindings.join("CMakeLists.txt").display()
    );
    for entry in fs::read_dir(source_dir.join("src")).expect("read sogen src directory") {
        let path = entry.expect("src entry").path();
        if path != rust_bindings {
            println!("cargo::rerun-if-changed={}", path.display());
        }
    }
}

// A static build links the icicle crate from Rust, since a second copy built as a staticlib would duplicate the Rust
// standard library. A shared build keeps icicle inside its backend library.
fn build_sogen(
    source_dir: &Path,
    out_dir: &Path,
    bridge_dir: &Path,
    static_build: bool,
) -> PathBuf {
    let switch = |enabled: bool| if enabled { "ON" } else { "OFF" };
    let static_crt = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|target_env| target_env == "msvc")
        && env::var("CARGO_CFG_TARGET_FEATURE")
            .is_ok_and(|features| features.split(',').any(|feature| feature == "crt-static"));
    if static_crt && !static_build {
        panic!("the `shared` feature cannot be used with a static C runtime (`+crt-static`)");
    }

    cmake::Config::new(source_dir)
        .profile(CMAKE_CONFIG)
        .out_dir(out_dir.join("cmake"))
        .build_target("sogen-rust-shim")
        .define("SOGEN_BUILD_STATIC", switch(static_build))
        .define("SOGEN_BUILD_ICICLE_BRIDGE", switch(!static_build))
        .define("SOGEN_BUILD_TOOLS", "OFF")
        .define("SOGEN_ENABLE_PYTHON_BINDINGS", "OFF")
        .define("SOGEN_ENABLE_RUST_BINDINGS", "ON")
        .define("SOGEN_RUST_BRIDGE_DIR", bridge_dir)
        .define("SOGEN_STATIC_CRT", switch(static_crt))
        .define(
            "SOGEN_ENABLE_SDL3",
            switch(env::var_os("CARGO_FEATURE_SDL").is_some()),
        )
        .build()
        .join("build")
}
