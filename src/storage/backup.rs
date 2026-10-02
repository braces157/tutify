//! Versioned, credential-free state backups and preview-bound restore transactions.
//! Callers hold Storage::lock while capturing, previewing, or applying state.

use super::*;
use crate::{mix::MixRecipes, stats::SongStats};
use anyhow::{bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::Read,
    time::{SystemTime, UNIX_EPOCH},
};

const FORMAT: &str = "tuitify-state-backup";
const VERSION: u32 = 1;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const JOURNAL_MAX_BYTES: usize = MAX_BYTES * 2;
const JOURNAL: &str = "restore-journal.json";
const FILES: [&str; 4] = [
    "config.json",
    "queue.json",
    "mix-recipes.json",
    "stats.json",
];
const JOURNAL_FILES: [&str; 5] = [
    "config.json",
    "queue.json",
    "mix-recipes.json",
    "stats.json",
    "cache.json",
];

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "state",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Snapshot<T> {
    Missing,
    Saved(T),
}
impl<T: Serialize> Snapshot<T> {
    fn bytes(&self) -> Result<Option<Vec<u8>>> {
        match self {
            Self::Missing => Ok(None),
            Self::Saved(value) => {
                // Value objects sort their keys, making comparisons independent
                // of HashMap insertion order and preserving duplicate arrays.
                let mut bytes = serde_json::to_vec_pretty(&serde_json::to_value(value)?)?;
                bytes.push(b'\n');
                ensure!(
                    bytes.len() <= MAX_BYTES,
                    "Saved-state snapshot exceeds 64 MiB"
                );
                Ok(Some(bytes))
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    format: String,
    version: u32,
    created_unix: u64,
    app_version: String,
    config: Snapshot<Config>,
    queue: Snapshot<Queue>,
    mix_recipes: Snapshot<MixRecipes>,
    stats: Snapshot<SongStats>,
}
impl Backup {
    fn validate(&self) -> Result<()> {
        ensure!(self.format == FORMAT, "Not a Tuitify state backup");
        ensure!(
            self.version == VERSION,
            "Unsupported backup version {}; preserve the backup and update Tuitify",
            self.version
        );
        ensure!(
            self.app_version.len() <= 64 && !self.app_version.chars().any(char::is_control),
            "Invalid backup build metadata"
        );
        if let Snapshot::Saved(config) = &self.config {
            ensure!(
                config.version == 1,
                "Unsupported config.json version {}",
                config.version
            );
            ensure!(
                config.volume <= 100 && config.background_dim <= 85,
                "Invalid config.json volume or background dim"
            );
        }
        if let Snapshot::Saved(queue) = &self.queue {
            queue.validate().context("Invalid queue.json in backup")?;
        }
        if let Snapshot::Saved(recipes) = &self.mix_recipes {
            recipes
                .validate()
                .context("Invalid mix-recipes.json in backup")?;
        }
        if let Snapshot::Saved(stats) = &self.stats {
            stats.validate().context("Invalid stats.json in backup")?;
        }
        Ok(())
    }

    fn summaries(&self) -> [String; 4] {
        [
            match &self.config {
                Snapshot::Missing => "no saved settings (defaults)".into(),
                Snapshot::Saved(c) => format!("settings v{}, volume {}%", c.version, c.volume),
            },
            match &self.queue {
                Snapshot::Missing => "no saved queue".into(),
                Snapshot::Saved(q) => format!(
                    "{} queue occurrences, position {} ms",
                    q.ids.len(),
                    q.position_ms
                ),
            },
            match &self.mix_recipes {
                Snapshot::Missing => "no saved recipes".into(),
                Snapshot::Saved(r) => format!("{} mix recipes", r.recipes.len()),
            },
            match &self.stats {
                Snapshot::Missing => "no saved statistics".into(),
                Snapshot::Saved(s) => format!("{} aggregate track entries", s.len()),
            },
        ]
    }
}

pub(super) fn extract_component(bytes: &[u8], name: &str) -> Result<Option<Vec<u8>>> {
    let backup: Backup = serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("Invalid whole-state backup; original files are preserved"))?;
    backup.validate()?;
    match name {
        "config.json" => backup.config.bytes(),
        "queue.json" => backup.queue.bytes(),
        "mix-recipes.json" => backup.mix_recipes.bytes(),
        "stats.json" => backup.stats.bytes(),
        _ => bail!(
            "Whole-state backups exclude replaceable cache; use a component backup or clear-cache"
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Create,
    Replace,
    Remove,
    Unchanged,
}
impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
struct RestoreFile {
    name: &'static str,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    change: Change,
    summary: String,
}
pub(crate) struct RestorePreview {
    root: PathBuf,
    source: PathBuf,
    source_hash: String,
    pub token: String,
    created_unix: u64,
    files: Vec<RestoreFile>,
}
impl fmt::Display for RestorePreview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Restore preview · backup v{VERSION} · created Unix {}",
            self.created_unix
        )?;
        writeln!(f, "Destination: {}", self.root.display())?;
        for file in &self.files {
            writeln!(f, "  {:16} {:9} {}", file.name, file.change, file.summary)?;
            if file.change != Change::Unchanged {
                writeln!(
                    f,
                    "    current: {} · backup: {}",
                    describe_bytes(&file.before),
                    describe_bytes(&file.after)
                )?;
            }
        }
        writeln!(
            f,
            "Credentials and replaceable cache are excluded and left unchanged."
        )?;
        writeln!(
            f,
            "Missing snapshots remove their saved file and restore its default state."
        )?;
        write!(f, "Confirmation token: {}", self.token)
    }
}

fn describe_bytes(bytes: &Option<Vec<u8>>) -> String {
    bytes.as_ref().map_or_else(
        || "absent".into(),
        |bytes| format!("{} bytes, SHA-256 {}", bytes.len(), hash(bytes)),
    )
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn confirmation_token(value: &str) -> std::result::Result<String, String> {
    if value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err("Use the 64-character confirmation token from the preview".into())
    }
}

pub(super) fn read_bytes(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("Cannot inspect {}", path.display()));
        }
    };
    ensure!(
        metadata.file_type().is_file(),
        "{} must be a regular file; it has been preserved",
        path.display()
    );
    ensure!(
        metadata.len() <= limit as u64,
        "{} exceeds the {} MiB limit; it has been preserved",
        path.display(),
        limit / (1024 * 1024)
    );
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "{} grew beyond the size limit",
        path.display()
    );
    Ok(Some(bytes))
}
fn capture<T: DeserializeOwned>(root: &Path, name: &str, total: &mut usize) -> Result<Snapshot<T>> {
    let Some(bytes) = read_bytes(&root.join(name), MAX_BYTES)? else {
        return Ok(Snapshot::Missing);
    };
    *total += bytes.len();
    ensure!(
        *total <= MAX_BYTES,
        "Saved state exceeds the combined 64 MiB backup limit"
    );
    Ok(Snapshot::Saved(
        serde_json::from_slice(&bytes).with_context(|| {
            format!("Cannot back up {name}; its original file has been preserved")
        })?,
    ))
}

/// Resolve parents without requiring a new output file to exist. Keep backup
/// artifacts outside the live state directory, including aliases through links.
pub(super) fn external_file(store: &Storage, path: &Path) -> Result<PathBuf> {
    let name = path.file_name().context("Supply a backup file path")?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent).context("Backup parent directory must exist")?;
    let root = fs::canonicalize(&store.root)?;
    let inside = if cfg!(windows) {
        PathBuf::from(parent.to_string_lossy().to_lowercase())
            .starts_with(PathBuf::from(root.to_string_lossy().to_lowercase()))
    } else {
        parent.starts_with(&root)
    };
    ensure!(
        !inside,
        "Keep backup files outside the live Tuitify data directory"
    );
    Ok(parent.join(name))
}

struct LimitedWrite<'a> {
    file: &'a mut tempfile::NamedTempFile,
    remaining: usize,
}
impl Write for LimitedWrite<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(std::io::Error::other("Snapshot exceeds its size limit"));
        }
        let written = self.file.write(bytes)?;
        self.remaining -= written;
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
fn json_temp<T: Serialize>(
    parent: &Path,
    value: &T,
    limit: usize,
) -> Result<tempfile::NamedTempFile> {
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut output = BufWriter::new(LimitedWrite {
            file: &mut temp,
            remaining: limit,
        });
        serde_json::to_writer_pretty(&mut output, value)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    temp.as_file().sync_all()?;
    Ok(temp)
}
pub(super) fn bytes_temp(parent: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile> {
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    Ok(temp)
}

impl Storage {
    pub(crate) fn backup(&self, destination: &Path) -> Result<()> {
        let destination = external_file(self, destination)?;
        let mut total = 0;
        let backup = Backup {
            format: FORMAT.into(),
            version: VERSION,
            created_unix: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            config: capture(&self.root, FILES[0], &mut total)?,
            queue: capture(&self.root, FILES[1], &mut total)?,
            mix_recipes: capture(&self.root, FILES[2], &mut total)?,
            stats: capture(&self.root, FILES[3], &mut total)?,
        };
        backup.validate()?;
        let temp = json_temp(destination.parent().unwrap(), &backup, MAX_BYTES)?;
        temp.persist_noclobber(&destination).map_err(|error| error.error)
            .context("Cannot create backup; choose a new file name (existing files are never overwritten)")?;
        Ok(())
    }

    pub(crate) fn restore_preview(&self, source: &Path) -> Result<RestorePreview> {
        let source = external_file(self, source)?;
        let bytes = read_bytes(&source, MAX_BYTES)?.context("Backup file does not exist")?;
        let backup: Backup = serde_json::from_slice(&bytes)
            .context("Invalid backup document; no saved-state files were changed")?;
        backup.validate()?;
        let incoming = [
            backup.config.bytes()?,
            backup.queue.bytes()?,
            backup.mix_recipes.bytes()?,
            backup.stats.bytes()?,
        ];
        ensure!(
            incoming.iter().flatten().map(Vec::len).sum::<usize>() <= MAX_BYTES,
            "Restored state exceeds 64 MiB"
        );
        let root = fs::canonicalize(&self.root)?;
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"tuitify-restore-preview-v1\0");
        let destination = root.to_string_lossy();
        fingerprint.update((destination.len() as u64).to_le_bytes());
        fingerprint.update(destination.as_bytes());
        fingerprint.update(Sha256::digest(&bytes));
        let mut files = Vec::new();
        let mut total = 0;
        for ((name, after), summary) in FILES.into_iter().zip(incoming).zip(backup.summaries()) {
            let component = super::recovery::StateFile::ALL
                .into_iter()
                .find(|file| file.name() == name)
                .unwrap();
            // Future snapshots need targeted recovery, which preserves their
            // original bytes in an external component backup before replacing.
            super::recovery::protect_future_version(self, component)?;
            let before = read_bytes(&root.join(name), MAX_BYTES)?;
            total += before.as_ref().map_or(0, Vec::len);
            ensure!(
                total <= MAX_BYTES,
                "Current saved state exceeds 64 MiB; it has been preserved"
            );
            fingerprint.update(name.as_bytes());
            fingerprint.update([u8::from(before.is_some())]);
            if let Some(before) = &before {
                fingerprint.update(Sha256::digest(before));
            }
            let change = match (&before, &after) {
                (None, None) => Change::Unchanged,
                (None, Some(_)) => Change::Create,
                (Some(_), None) => Change::Remove,
                (Some(old), Some(new))
                    if serde_json::from_slice::<serde_json::Value>(old).ok()
                        == serde_json::from_slice::<serde_json::Value>(new).ok() =>
                {
                    Change::Unchanged
                }
                _ => Change::Replace,
            };
            files.push(RestoreFile {
                name,
                before,
                after,
                change,
                summary,
            });
        }
        Ok(RestorePreview {
            root,
            source,
            source_hash: hash(&bytes),
            token: format!("{:x}", fingerprint.finalize()),
            created_unix: backup.created_unix,
            files,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    committed: bool,
    files: Vec<JournalFile>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalFile {
    name: String,
    #[serde(deserialize_with = "required_option")]
    before_base64: Option<String>,
    #[serde(deserialize_with = "required_option")]
    after_hash: Option<String>,
}
fn required_option<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}

impl RestorePreview {
    pub(crate) fn apply(self, store: &Storage, token: &str) -> Result<()> {
        ensure!(
            confirmation_token(token).ok().as_deref() == Some(self.token.as_str()),
            "Restore preview changed or confirmation token is incorrect; inspect a fresh preview. No files were changed"
        );
        ensure!(
            fs::canonicalize(&store.root)? == self.root,
            "Restore destination changed; inspect a fresh preview"
        );
        let source = read_bytes(&self.source, MAX_BYTES)?
            .context("Backup disappeared; no files were changed")?;
        ensure!(
            hash(&source) == self.source_hash,
            "Backup changed; inspect a fresh preview. No files were changed"
        );
        ensure!(
            !self.root.join(JOURNAL).exists(),
            "A restore journal already exists; reopen Tuitify to recover it first"
        );
        for file in &self.files {
            ensure!(
                read_bytes(&self.root.join(file.name), MAX_BYTES)? == file.before,
                "{} changed; inspect a fresh preview. No files were changed",
                file.name
            );
        }
        apply_files(store, &self.root, &self.files)
    }
}

/// Reuse the existing durable restore transaction for a selected recovery file.
pub(super) fn apply_component(
    store: &Storage,
    name: &'static str,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
) -> Result<()> {
    ensure!(JOURNAL_FILES.contains(&name), "Invalid recovery component");
    let root = fs::canonicalize(&store.root)?;
    let change = if before == after {
        Change::Unchanged
    } else if after.is_none() {
        Change::Remove
    } else if before.is_none() {
        Change::Create
    } else {
        Change::Replace
    };
    apply_files(
        store,
        &root,
        &[RestoreFile {
            name,
            before,
            after,
            change,
            summary: String::new(),
        }],
    )
}

fn apply_files(store: &Storage, root: &Path, files: &[RestoreFile]) -> Result<()> {
    ensure!(
        !root.join(JOURNAL).exists(),
        "A restore journal already exists; reopen Tuitify to recover it first"
    );
    for file in files {
        ensure!(
            read_bytes(&root.join(file.name), MAX_BYTES)? == file.before,
            "{} changed; inspect a fresh preview. No files were changed",
            file.name
        );
    }
    let changed: Vec<_> = files
        .iter()
        .filter(|file| file.change != Change::Unchanged)
        .collect();
    if changed.is_empty() {
        return Ok(());
    }
    // Stage and sync every replacement before touching any live state.
    let mut staged = Vec::new();
    for file in &changed {
        staged.push(
            file.after
                .as_ref()
                .map(|bytes| bytes_temp(root, bytes))
                .transpose()?,
        );
    }
    let mut journal = Journal {
        version: VERSION,
        committed: false,
        files: changed
            .iter()
            .map(|file| JournalFile {
                name: file.name.into(),
                before_base64: file.before.as_ref().map(|bytes| STANDARD.encode(bytes)),
                after_hash: file.after.as_ref().map(|bytes| hash(bytes)),
            })
            .collect(),
    };
    json_temp(root, &journal, JOURNAL_MAX_BYTES)?
        .persist_noclobber(root.join(JOURNAL))
        .map_err(|e| e.error)?;
    let publish = (|| -> Result<()> {
        for (file, temp) in changed.iter().zip(staged) {
            ensure!(
                read_bytes(&root.join(file.name), MAX_BYTES)? == file.before,
                "{} changed during restore",
                file.name
            );
            if let Some(temp) = temp {
                temp.persist(root.join(file.name)).map_err(|e| e.error)?;
            } else {
                fs::remove_file(root.join(file.name))?;
            }
        }
        journal.committed = true;
        let temp = json_temp(root, &journal, JOURNAL_MAX_BYTES)?;
        temp.persist(root.join(JOURNAL)).map_err(|e| e.error)?;
        Ok(())
    })();
    if let Err(error) = publish {
        return match recover_pending(store) {
            Ok(()) => {
                Err(error).context("Restore failed; original saved-state files were recovered")
            }
            Err(recovery) => Err(error).context(format!(
                "Restore failed; recovery journal preserved: {recovery:#}"
            )),
        };
    }
    fs::remove_file(root.join(JOURNAL))
        .context("Restore applied; journal cleanup failed (the next launch will clean it up)")?;
    Ok(())
}

/// Called only after acquiring the instance lock, before loading state. A
/// committed journal is cleanup-only; an interrupted transaction rolls back.
pub(super) fn recover_pending(store: &Storage) -> Result<()> {
    let path = store.root.join(JOURNAL);
    let Some(bytes) = read_bytes(&path, JOURNAL_MAX_BYTES)? else {
        return Ok(());
    };
    let journal: Journal = serde_json::from_slice(&bytes).context(
        "Invalid restore journal; preserve restore-journal.json before repairing saved state",
    )?;
    ensure!(
        journal.version == VERSION
            && !journal.files.is_empty()
            && journal.files.len() <= JOURNAL_FILES.len(),
        "Unsupported restore journal; it has been preserved"
    );
    let mut seen = std::collections::HashSet::new();
    let mut originals = Vec::new();
    let mut total = 0;
    for file in &journal.files {
        ensure!(
            JOURNAL_FILES.contains(&file.name.as_str()) && seen.insert(&file.name),
            "Invalid file in restore journal; it has been preserved"
        );
        let before = file
            .before_base64
            .as_ref()
            .map(|bytes| STANDARD.decode(bytes))
            .transpose()
            .context("Invalid recovery data; journal preserved")?;
        total += before.as_ref().map_or(0, Vec::len);
        ensure!(
            total <= MAX_BYTES,
            "Restore journal originals exceed the size limit"
        );
        ensure!(
            file.after_hash
                .as_ref()
                .is_none_or(|hash| confirmation_token(hash).is_ok()),
            "Invalid restore journal hash"
        );
        let mut needs_restore = false;
        if !journal.committed {
            let current = read_bytes(&store.root.join(&file.name), MAX_BYTES)?;
            let before_hash = before.as_ref().map(|bytes| hash(bytes));
            let current_hash = current.as_ref().map(|bytes| hash(bytes));
            ensure!(
                current_hash == before_hash || current_hash == file.after_hash,
                "{} changed outside the interrupted restore; preserve restore-journal.json and this file before recovery",
                file.name
            );
            needs_restore = current_hash != before_hash;
        }
        originals.push((before, needs_restore));
    }
    if !journal.committed {
        let mut staged = Vec::new();
        for (before, needed) in &originals {
            staged.push(if *needed {
                before
                    .as_ref()
                    .map(|bytes| bytes_temp(&store.root, bytes))
                    .transpose()?
            } else {
                None
            });
        }
        for ((file, temp), (_, needed)) in journal.files.iter().zip(staged).zip(originals) {
            if !needed {
                continue;
            }
            let destination = store.root.join(&file.name);
            if let Some(temp) = temp {
                temp.persist(destination).map_err(|e| e.error)?;
            } else if destination.exists() {
                fs::remove_file(destination)?;
            }
        }
    }
    fs::remove_file(path)
        .context("Recovered restore state; could not remove the restore journal")?;
    Ok(())
}

#[cfg(test)]
mod tests;
