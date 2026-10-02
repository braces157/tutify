//! Shared build-source accounting. No source contents or local paths are embedded.
use sha2::{Digest, Sha256};
use std::{fs, io, path::Path, process::Command};

pub fn fingerprint(fields: &[(&str, &str)]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"tuitify-build-identity-v1\0");
    for (name, value) in fields {
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

pub fn source_digest(root: &Path) -> io::Result<String> {
    fn files(root: &Path, path: &Path, output: &mut Vec<String>) -> io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::other("Build inputs must not be symlinks"));
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                files(root, &entry?.path(), output)?;
            }
        } else if metadata.is_file() {
            let name = path.strip_prefix(root).map_err(io::Error::other)?;
            output.push(
                name.to_str()
                    .ok_or_else(|| io::Error::other("Non-UTF-8 build input"))?
                    .replace('\\', "/"),
            );
        } else {
            return Err(io::Error::other("Unsupported build input"));
        }
        Ok(())
    }
    let mut inputs = vec![
        "Cargo.toml".into(),
        "Cargo.lock".into(),
        "build.rs".into(),
        "build_support.rs".into(),
    ];
    files(root, &root.join("src"), &mut inputs)?;
    inputs.sort();
    let mut hash = Sha256::new();
    hash.update(b"tuitify-source-tree-v1\0");
    for name in inputs {
        let path = root.join(&name);
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(io::Error::other("Build inputs must not be symlinks"));
        }
        let bytes = fs::read(path)?;
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
        .map(|text| text.trim().to_owned())
}

pub fn git_identity(root: &Path) -> (Option<String>, Option<bool>) {
    let Some(top) = git(root, &["rev-parse", "--show-toplevel"]) else {
        return (None, None);
    };
    // A source archive nested inside another checkout is not that checkout's source.
    if fs::canonicalize(top).ok() != fs::canonicalize(root).ok() {
        return (None, None);
    }
    let commit = git(root, &["rev-parse", "--verify", "HEAD"]).filter(|value| {
        matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    let dirty = git(
        root,
        &[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            "Cargo.toml",
            "Cargo.lock",
            "build.rs",
            "build_support.rs",
            "src",
        ],
    )
    .map(|text| !text.is_empty());
    (commit, dirty)
}
