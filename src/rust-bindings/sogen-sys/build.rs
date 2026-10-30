use std::path::Path;
use std::{env, fs};

fn main() {
    let manifest = env::var("DEP_SOGEN_NATIVE_LINK_MANIFEST")
        .expect("DEP_SOGEN_NATIVE_LINK_MANIFEST set by sogen-native");
    println!("cargo::rerun-if-changed={manifest}");

    let content = fs::read_to_string(&manifest)
        .unwrap_or_else(|error| panic!("read link manifest {manifest}: {error}"));
    for line in content.lines().filter(|line| !line.is_empty()) {
        let (kind, value) = line
            .split_once('=')
            .unwrap_or_else(|| panic!("malformed link manifest line: {line}"));
        if value.is_empty() {
            continue;
        }

        match kind {
            "bundle" => link_static(Path::new(value), ""),
            "static" => link_static(Path::new(value), ":-bundle"),
            "shared" => link_shared(Path::new(value)),
            "item" => link_item(value),
            _ => panic!("unknown link manifest entry kind: {kind}"),
        }
    }
}

// The shim is bundled into this crate's rlib because its bridge code calls back into the rlib, which single-pass
// linkers only resolve while scanning the rlib itself.
fn link_static(path: &Path, modifiers: &str) {
    let dir = path.parent().expect("static library directory");
    println!("cargo::rustc-link-search=native={}", dir.display());
    println!(
        "cargo::rustc-link-lib=static{modifiers}={}",
        library_name(path)
    );
}

fn link_shared(path: &Path) {
    let dir = path.parent().expect("shared library directory");
    println!("cargo::rustc-link-search=native={}", dir.display());
    println!("cargo::rustc-link-lib=dylib={}", library_name(path));
}

fn link_item(item: &str) {
    if let Some(framework) = item.strip_prefix("-framework ") {
        println!("cargo::rustc-link-lib=framework={}", framework.trim());
    } else if let Some(name) = item.strip_prefix("-l") {
        println!("cargo::rustc-link-lib={name}");
    } else if item == "-pthread" {
        println!("cargo::rustc-link-lib=pthread");
    } else if item.starts_with('-') {
        println!("cargo::warning=sogen-sys ignores the link option {item}");
    } else if Path::new(item).is_absolute() {
        let path = Path::new(item);
        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase());
        match extension.as_deref() {
            Some("a" | "lib") => link_static(path, ":-bundle"),
            _ => link_shared(path),
        }
    } else {
        println!(
            "cargo::rustc-link-lib={}",
            item.strip_suffix(".lib").unwrap_or(item)
        );
    }
}

fn library_name(path: &Path) -> String {
    let file = path.file_name().expect("library file").to_string_lossy();
    let stem = file.split('.').next().expect("library stem");
    let unix_library = path.extension().is_some_and(|extension| extension != "lib");
    let name = match stem.strip_prefix("lib") {
        Some(name) if unix_library => name,
        _ => stem,
    };
    name.to_owned()
}
