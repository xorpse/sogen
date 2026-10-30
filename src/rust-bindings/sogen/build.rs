use std::env;

// Forwards the runtime directories under `links = "sogen"`, so packages using `sogen_build` depend on `sogen` instead of
// `sogen-native`.
fn main() {
    let runtime_dirs = env::join_paths(sogen_build::runtime_dirs())
        .expect("join runtime dirs")
        .into_string()
        .expect("runtime directories are UTF-8");
    println!("cargo::metadata=runtime_dirs={runtime_dirs}");

    sogen_build::configure_linkage();
}
