#[path = "../build_support.rs"]
mod build_support;
use std::{fs, path::Path, process::Command};

fn source(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    for name in [
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "build_support.rs",
        "src/main.rs",
    ] {
        fs::write(root.join(name), format!("fixture {name}\n")).unwrap();
    }
}

#[test]
fn source_digest_tracks_edits_additions_deletions_and_names_but_not_location_or_docs() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    source(first.path());
    source(second.path());
    let original = build_support::source_digest(first.path()).unwrap();
    assert_eq!(
        original,
        build_support::source_digest(second.path()).unwrap()
    );
    fs::write(first.path().join("README.md"), "outside compiled inputs").unwrap();
    assert_eq!(
        original,
        build_support::source_digest(first.path()).unwrap()
    );
    fs::write(first.path().join("src/main.rs"), "changed source").unwrap();
    assert_ne!(
        original,
        build_support::source_digest(first.path()).unwrap()
    );
    source(first.path());
    fs::write(first.path().join("src/extra.rs"), "new input").unwrap();
    assert_ne!(
        original,
        build_support::source_digest(first.path()).unwrap()
    );
    fs::remove_file(first.path().join("src/extra.rs")).unwrap();
    assert_eq!(
        original,
        build_support::source_digest(first.path()).unwrap()
    );
    fs::rename(
        first.path().join("src/main.rs"),
        first.path().join("src/renamed.rs"),
    )
    .unwrap();
    assert_ne!(
        original,
        build_support::source_digest(first.path()).unwrap()
    );
}

#[test]
fn fingerprint_has_framed_fields_and_distinguishes_same_version_source_and_compiler() {
    assert_ne!(
        build_support::fingerprint(&[("ab", "c")]),
        build_support::fingerprint(&[("a", "bc")])
    );
    let first =
        build_support::fingerprint(&[("version", "0.3.1"), ("source", "first"), ("rustc", "1.88")]);
    assert_ne!(
        first,
        build_support::fingerprint(&[
            ("version", "0.3.1"),
            ("source", "second"),
            ("rustc", "1.88")
        ])
    );
    assert_ne!(
        first,
        build_support::fingerprint(&[("version", "0.3.1"), ("source", "first"), ("rustc", "1.95")])
    );
}

#[test]
fn missing_input_fails_instead_of_hashing_an_incomplete_tree() {
    let root = tempfile::tempdir().unwrap();
    source(root.path());
    fs::remove_file(root.path().join("Cargo.lock")).unwrap();
    assert!(build_support::source_digest(root.path()).is_err());
}

#[test]
fn source_archives_never_inherit_the_enclosing_repository_identity() {
    let root = tempfile::tempdir().unwrap();
    source(root.path());
    assert_eq!(build_support::git_identity(root.path()), (None, None));
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(root.path())
            .status()
            .unwrap()
            .success()
    );
    let archive = root.path().join("nested-archive");
    fs::create_dir(&archive).unwrap();
    source(&archive);
    assert_eq!(build_support::git_identity(&archive), (None, None));
}

#[test]
fn clean_dirty_and_untracked_source_are_observed_without_paths_in_identity() {
    let root = tempfile::tempdir().unwrap();
    source(root.path());
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(root.path())
            .status()
            .unwrap()
            .success()
    );
    assert!(build_support::git(root.path(), &["config", "core.autocrlf", "false"]).is_some());
    assert!(build_support::git(root.path(), &["add", "."]).is_some());
    assert!(
        build_support::git(
            root.path(),
            &[
                "-c",
                "user.name=Build fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=NUL",
                "commit",
                "-qm",
                "fixture"
            ]
        )
        .is_some()
    );
    let (commit, dirty) = build_support::git_identity(root.path());
    assert!(matches!(commit.as_ref().unwrap().len(), 40 | 64));
    assert_eq!(dirty, Some(false));
    fs::write(
        root.path().join("README.md"),
        "documentation does not dirty compiled source",
    )
    .unwrap();
    assert_eq!(
        build_support::git_identity(root.path()),
        (commit.clone(), Some(false))
    );
    fs::write(root.path().join("src/untracked.rs"), "new compiled input").unwrap();
    assert_eq!(
        build_support::git_identity(root.path()),
        (commit.clone(), Some(true))
    );
    fs::remove_file(root.path().join("src/untracked.rs")).unwrap();
    fs::write(root.path().join("src/main.rs"), "changed tracked source").unwrap();
    assert_eq!(
        build_support::git_identity(root.path()),
        (commit, Some(true))
    );
}

#[test]
fn version_json_runs_before_saved_state_or_authentication_and_hashes_the_actual_binary() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let missing_profile = root.path().join("no-profile");
    let exe = env!("CARGO_BIN_EXE_tuitify");
    let output = Command::new(exe)
        .args(["version", "--json"])
        .env("LOCALAPPDATA", &missing_profile)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(!missing_profile.exists());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["format"], "tuitify-build");
    assert_eq!(report["format_version"], 1);
    assert_eq!(report["build"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        report["on_disk_executable_sha256"],
        format!("{:x}", Sha256::digest(fs::read(exe).unwrap()))
    );
    for key in ["source_sha256", "build_id", "settings_sha256"] {
        assert_eq!(report["build"][key].as_str().unwrap().len(), 64);
    }
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains(&root.path().to_string_lossy().to_string())
    );
}

#[test]
fn windows_powershell_source_checker_agrees_with_rust_for_unicode_and_binary_inputs() {
    use base64::Engine;
    let root = tempfile::tempdir().unwrap();
    source(root.path());
    for name in ["src/😀.rs", "src/\u{e000}.rs", "src/日本語.rs"] {
        fs::write(root.path().join(name), [0, 1, 255, 13, 10]).unwrap();
    }
    let helper = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/build-manifest.ps1");
    let quote = |path: &Path| path.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; . '{}'; \
         $taskRoot='{}'; \
         Get-TuitifySourceDigest -ProjectRoot $taskRoot; \
         $taskShort=(New-Object -ComObject Scripting.FileSystemObject).GetFolder($taskRoot).ShortPath; \
         Get-TuitifySourceDigest -ProjectRoot $taskShort; \
         Get-TuitifySourceDigest -ProjectRoot ((Get-Item -LiteralPath $taskRoot).FullName + '\\')",
        quote(&helper),
        quote(root.path())
    );
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = build_support::source_digest(root.path()).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let hashes: Vec<_> = stdout.lines().map(str::trim).collect();
    assert_eq!(hashes, vec![expected.as_str(); 3]);
}
