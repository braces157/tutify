mod build_support;
use std::{env, path::PathBuf, process::Command};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"));
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "build_support.rs",
        "src",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    for path in ["HEAD", "index", "refs", "packed-refs"] {
        if let Some(path) = build_support::git(&root, &["rev-parse", "--git-path", path])
            && root.join(&path).exists()
        {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    // Detect creation/removal of Git metadata for standalone source distributions.
    println!("cargo:rerun-if-changed=.git");
    println!("cargo:rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
    let source = build_support::source_digest(&root).expect("Hash complete build source");
    let (commit, dirty) = build_support::git_identity(&root);
    let compiler = Command::new(env::var_os("RUSTC").expect("Cargo Rust compiler"))
        .arg("--version")
        .output()
        .expect("Read compiler identity");
    assert!(compiler.status.success(), "Compiler identity failed");
    let compiler = String::from_utf8(compiler.stdout).expect("UTF-8 compiler identity");
    let compiler = compiler.trim();
    assert!(
        !compiler.contains(['\n', '\r']),
        "Single-line compiler identity required"
    );
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let target = env::var("TARGET").unwrap();
    let profile = env::var("PROFILE").unwrap();
    let flags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    let opt = env::var("OPT_LEVEL").unwrap();
    let debug = env::var("DEBUG").unwrap();
    let settings = build_support::fingerprint(&[
        ("rustflags", &flags),
        ("opt_level", &opt),
        ("debug", &debug),
    ]);
    let dirty = dirty.map_or("unknown", |dirty| if dirty { "true" } else { "false" });
    let commit = commit.as_deref().unwrap_or("");
    let id = build_support::fingerprint(&[
        ("version", &version),
        ("source", &source),
        ("commit", commit),
        ("source_dirty", dirty),
        ("target", &target),
        ("profile", &profile),
        ("compiler", compiler),
        ("settings", &settings),
    ]);
    for (name, value) in [
        ("SOURCE_SHA256", source.as_str()),
        ("SOURCE_COMMIT", commit),
        ("SOURCE_DIRTY", dirty),
        ("TARGET", &target),
        ("PROFILE", &profile),
        ("RUSTC", compiler),
        ("SETTINGS_SHA256", &settings),
        ("ID", &id),
    ] {
        println!("cargo:rustc-env=TUITIFY_BUILD_{name}={value}");
    }
}
