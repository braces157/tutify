//! Read-only inspection and preview-bound recovery of exactly one state file.
//! Production callers hold the instance lock; credentials are never accessed.

use super::{Config, Storage, backup};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use clap::ValueEnum;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const BACKUP_MAX_BYTES: usize = 90 * 1024 * 1024;
const FORMAT: &str = "tuitify-component-backup";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StateFile {
    #[value(alias = "config.json")]
    Config,
    #[value(alias = "queue.json")]
    Queue,
    #[value(alias = "mix-recipes", alias = "mix-recipes.json")]
    Recipes,
    #[value(alias = "stats.json")]
    Stats,
    #[value(alias = "cache.json")]
    Cache,
}
impl StateFile {
    pub const ALL: [Self; 5] = [
        Self::Config,
        Self::Queue,
        Self::Recipes,
        Self::Stats,
        Self::Cache,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Config => "config.json",
            Self::Queue => "queue.json",
            Self::Recipes => "mix-recipes.json",
            Self::Stats => "stats.json",
            Self::Cache => "cache.json",
        }
    }
    pub fn argument(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Queue => "queue",
            Self::Recipes => "recipes",
            Self::Stats => "stats",
            Self::Cache => "cache",
        }
    }
    fn validate(self, bytes: &[u8]) -> Result<String> {
        match self {
            Self::Config => {
                let config: Config = decode(self, bytes)?;
                validate_config(&config).map_err(|error| invariant(self, error.to_string()))?;
                Ok(format!("settings v1, volume {}%", config.volume))
            }
            Self::Queue => {
                let queue: crate::queue::Queue = decode(self, bytes)?;
                queue
                    .validate()
                    .map_err(|error| invariant(self, error.to_string()))?;
                Ok(format!(
                    "{} queue occurrences, position {} ms",
                    queue.ids.len(),
                    queue.position_ms
                ))
            }
            Self::Recipes => {
                let recipes: crate::mix::MixRecipes = decode(self, bytes)?;
                recipes
                    .validate()
                    .map_err(|error| invariant(self, error.to_string()))?;
                Ok(format!("{} saved mix recipes", recipes.recipes.len()))
            }
            Self::Stats => {
                let stats: crate::stats::SongStats = decode(self, bytes)?;
                validate_stats(&stats).map_err(|error| invariant(self, error.to_string()))?;
                Ok(format!("{} aggregate track entries", stats.len()))
            }
            Self::Cache => {
                let mut cache: crate::cache::MetadataCache = decode(self, bytes)?;
                cache
                    .validate()
                    .map_err(|error| invariant(self, error.to_string()))?;
                Ok("replaceable metadata cache (expired entries are ignored)".into())
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StateFailureKind {
    Io,
    Malformed,
    UnsupportedVersion(u64),
    Invariant,
}
#[derive(Debug)]
pub(crate) struct StateFailure {
    pub file: StateFile,
    pub kind: StateFailureKind,
    pub detail: String,
}
impl fmt::Display for StateFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let category = match self.kind {
            StateFailureKind::Io => "read error".into(),
            StateFailureKind::Malformed => "invalid JSON/schema".into(),
            StateFailureKind::UnsupportedVersion(version) => format!("unsupported v{version}"),
            StateFailureKind::Invariant => "invalid state".into(),
        };
        write!(
            f,
            "{} [{category}]: {} (file preserved). Recovery: tuitify state inspect {}; tuitify state backup {} FILE; tuitify state restore {} FILE; or tuitify state reset {} for a preview.",
            self.file.name(),
            self.detail,
            self.file.argument(),
            self.file.argument(),
            self.file.argument(),
            self.file.argument()
        )
    }
}
impl std::error::Error for StateFailure {}

pub(super) fn invariant(file: StateFile, detail: String) -> StateFailure {
    StateFailure {
        file,
        kind: StateFailureKind::Invariant,
        detail,
    }
}
fn malformed(file: StateFile, detail: String) -> StateFailure {
    StateFailure {
        file,
        kind: StateFailureKind::Malformed,
        detail,
    }
}
fn document(file: StateFile, bytes: &[u8]) -> Result<serde_json::Value> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
        malformed(
            file,
            format!(
                "Invalid JSON at line {}, column {}",
                error.line(),
                error.column()
            ),
        )
    })?;
    ensure!(
        value.is_object(),
        malformed(file, "State must be a JSON object".into())
    );
    if let Some(version) = value.get("version") {
        let version = version
            .as_u64()
            .ok_or_else(|| malformed(file, "Version must be an unsigned integer".into()))?;
        ensure!(
            version == 1,
            StateFailure {
                file,
                kind: StateFailureKind::UnsupportedVersion(version),
                detail: format!(
                    "Unsupported version {version}; update Tuitify before loading this file"
                )
            }
        );
    }
    Ok(value)
}
pub(super) fn decode<T: DeserializeOwned>(file: StateFile, bytes: &[u8]) -> Result<T> {
    document(file, bytes)?;
    serde_json::from_slice(bytes).map_err(|error| {
        malformed(
            file,
            format!(
                "Invalid field type or missing required field at line {}, column {}",
                error.line(),
                error.column()
            ),
        )
        .into()
    })
}
pub(super) fn read_file(store: &Storage, file: StateFile) -> Result<Option<Vec<u8>>> {
    backup::read_bytes(&store.root.join(file.name()), MAX_BYTES).map_err(|_| StateFailure {
        file, kind: StateFailureKind::Io,
        detail: "Cannot read a regular state file within the 64 MiB limit; check permissions, file type, and size".into(),
    }.into())
}
pub(super) fn load<T: DeserializeOwned + Default>(store: &Storage, file: StateFile) -> Result<T> {
    read_file(store, file)?
        .map(|bytes| decode(file, &bytes))
        .transpose()
        .map(Option::unwrap_or_default)
}
pub(super) fn protect_future_version(store: &Storage, file: StateFile) -> Result<()> {
    if let Some(bytes) = read_file(store, file)? {
        // Corrupted replaceable caches may be regenerated. A recognizable
        // unsupported version must be preserved even when its shape is unknown.
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && value.get("version").is_some()
        {
            document(file, &bytes)?;
        }
    }
    Ok(())
}
pub(super) fn validate_config(config: &Config) -> Result<()> {
    ensure!(config.volume <= 100, "Volume must be at most 100");
    ensure!(
        config.background_dim <= 85,
        "Background dim must be at most 85"
    );
    Ok(())
}
pub(super) fn validate_stats(stats: &crate::stats::SongStats) -> Result<()> {
    ensure!(
        stats.tracks.len() <= crate::stats::MAX_STATS_ENTRIES,
        "Statistics exceed 50,000 entries"
    );
    ensure!(
        stats
            .tracks
            .iter()
            .all(|(key, stat)| crate::model::valid_id(key) && key == &stat.id),
        "Statistics track keys must be valid IDs matching their entry ID"
    );
    Ok(())
}

pub(crate) struct StateInspection {
    pub file: StateFile,
    pub bytes: Option<usize>,
    pub version: Option<u64>,
    pub result: Result<String>,
}
impl fmt::Display for StateInspection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.result {
            Ok(summary) => write!(f, "{}: OK · {}", self.file.name(), summary),
            Err(error) => write!(f, "{error:#}"),
        }?;
        if let Some(bytes) = self.bytes {
            write!(f, " · {bytes} bytes")?;
        }
        if let Some(version) = self.version {
            write!(f, " · version {version}")?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentBackup {
    format: String,
    version: u32,
    component: StateFile,
    sha256: String,
    bytes_base64: String,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn envelope(file: StateFile, bytes: &[u8]) -> Result<Vec<u8>> {
    let backup = ComponentBackup {
        format: FORMAT.into(),
        version: 1,
        component: file,
        sha256: hash(bytes),
        bytes_base64: STANDARD.encode(bytes),
    };
    let bytes = serde_json::to_vec_pretty(&backup)?;
    ensure!(
        bytes.len() <= BACKUP_MAX_BYTES,
        "Component backup exceeds 90 MiB"
    );
    Ok(bytes)
}
fn write_backup(
    store: &Storage,
    file: StateFile,
    bytes: &[u8],
    destination: &Path,
    reuse: bool,
) -> Result<()> {
    let destination = backup::external_file(store, destination)?;
    let serialized = envelope(file, bytes)?;
    if reuse
        && backup::read_bytes(&destination, BACKUP_MAX_BYTES)?.as_deref()
            == Some(serialized.as_slice())
    {
        return Ok(());
    }
    backup::bytes_temp(destination.parent().unwrap(), &serialized)?
        .persist_noclobber(&destination)
        .map_err(|error| error.error)
        .context("Cannot create component backup; existing files are never overwritten")?;
    Ok(())
}
fn extract(bytes: &[u8], file: StateFile) -> Result<Option<Vec<u8>>> {
    ensure!(
        bytes.len() <= BACKUP_MAX_BYTES,
        "Component backup exceeds 90 MiB"
    );
    let header: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("Invalid backup JSON; no files were changed"))?;
    if header["format"] == "tuitify-state-backup" {
        return backup::extract_component(bytes, file.name());
    }
    let backup: ComponentBackup = serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("Invalid component backup; no files were changed"))?;
    ensure!(
        backup.format == FORMAT && backup.version == 1,
        "Unsupported component backup; preserve it and update Tuitify"
    );
    ensure!(
        backup.component == file,
        "Backup belongs to a different state file; no files were changed"
    );
    ensure!(
        backup.bytes_base64.len() <= MAX_BYTES.div_ceil(3) * 4,
        "Decoded component exceeds 64 MiB"
    );
    let bytes = STANDARD
        .decode(backup.bytes_base64)
        .context("Invalid component backup encoding")?;
    ensure!(
        bytes.len() <= MAX_BYTES && hash(&bytes) == backup.sha256,
        "Component backup integrity check failed; no files were changed"
    );
    Ok(Some(bytes))
}

pub(crate) struct RecoveryPreview {
    root: PathBuf,
    file: StateFile,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    source: Option<(PathBuf, String)>,
    archive: Option<PathBuf>,
    pub token: String,
    summary: String,
}
impl fmt::Display for RecoveryPreview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}: {}", self.file.name(), self.summary)?;
        writeln!(
            f,
            "Destination: {}",
            self.root.join(self.file.name()).display()
        )?;
        writeln!(
            f,
            "Current: {}",
            self.before.as_ref().map_or_else(
                || "absent".into(),
                |bytes| format!("{} bytes, SHA-256 {}", bytes.len(), hash(bytes))
            )
        )?;
        if let Some(after) = &self.after {
            writeln!(
                f,
                "Incoming: {} bytes, SHA-256 {}",
                after.len(),
                hash(after)
            )?;
        }
        if let Some(path) = &self.archive {
            writeln!(f, "Original-file backup: {}", path.display())?;
        }
        writeln!(f, "Other state files and credentials are left unchanged.")?;
        write!(f, "Confirmation token: {}", self.token)
    }
}
impl RecoveryPreview {
    pub(crate) fn apply(self, store: &Storage, confirmation: &str) -> Result<()> {
        ensure!(
            super::confirmation_token(confirmation).ok().as_deref() == Some(&self.token),
            "Recovery preview changed or confirmation is incorrect; inspect a fresh preview. No files were changed"
        );
        ensure!(
            fs::canonicalize(&store.root)? == self.root,
            "Recovery destination changed; no files were changed"
        );
        ensure!(
            read_file(store, self.file)? == self.before,
            "Selected state file changed; inspect a fresh preview"
        );
        if let Some((source, expected)) = &self.source {
            let bytes = backup::read_bytes(source, BACKUP_MAX_BYTES)?
                .context("Recovery backup disappeared")?;
            ensure!(
                hash(&bytes) == *expected,
                "Recovery backup changed; inspect a fresh preview"
            );
        }
        if self.before == self.after {
            return Ok(());
        }
        if let (Some(bytes), Some(archive)) = (&self.before, &self.archive) {
            write_backup(store, self.file, bytes, archive, true)?;
        }
        backup::apply_component(store, self.file.name(), self.before, self.after)
    }
}

impl Storage {
    pub(crate) fn inspect_state(&self, file: StateFile) -> StateInspection {
        match read_file(self, file) {
            Ok(Some(bytes)) => StateInspection {
                file,
                bytes: Some(bytes.len()),
                version: serde_json::from_slice::<serde_json::Value>(&bytes)
                    .ok()
                    .and_then(|value| value["version"].as_u64()),
                result: file.validate(&bytes),
            },
            Ok(None) => StateInspection {
                file,
                bytes: None,
                version: None,
                result: Ok("not saved; defaults apply".into()),
            },
            Err(error) => StateInspection {
                file,
                bytes: None,
                version: None,
                result: Err(error),
            },
        }
    }
    pub(crate) fn backup_component(&self, file: StateFile, destination: &Path) -> Result<()> {
        let bytes = read_file(self, file)?
            .context("Selected state file is not saved; there is nothing to back up")?;
        write_backup(self, file, &bytes, destination, false)
    }
    pub(crate) fn recovery_preview(
        &self,
        file: StateFile,
        source: Option<&Path>,
        archive: Option<&Path>,
    ) -> Result<RecoveryPreview> {
        let root = fs::canonicalize(&self.root)?;
        let before = read_file(self, file)?;
        let (after, source, summary) = if let Some(source) = source {
            let source = backup::external_file(self, source)?;
            let bytes = backup::read_bytes(&source, BACKUP_MAX_BYTES)?
                .context("Recovery backup does not exist")?;
            let after = extract(&bytes, file)?;
            let summary = match &after {
                Some(bytes) => format!("restore {}", file.validate(bytes)?),
                None => "restore missing snapshot (defaults)".into(),
            };
            (after, Some((source, hash(&bytes))), summary)
        } else {
            (
                None,
                None,
                "reset this saved file to its default state".into(),
            )
        };
        let archive = if let Some(bytes) = before.as_ref().filter(|_| before != after) {
            let default = root
                .parent()
                .context("Data directory needs a parent for recovery backups")?
                .join(format!(
                    "tuitify-recovery-{}-{}.json",
                    file.argument(),
                    hash(bytes)
                ));
            Some(backup::external_file(self, archive.unwrap_or(&default))?)
        } else {
            None
        };
        if let Some((source, _)) = &source {
            ensure!(
                archive.as_ref() != Some(source),
                "Use different paths for the restore source and original-file backup"
            );
        }
        let mut fingerprint = Sha256::new();
        fingerprint.update(b"tuitify-component-recovery-v1\0");
        for value in [
            root.to_string_lossy().as_bytes(),
            file.name().as_bytes(),
            summary.as_bytes(),
        ] {
            fingerprint.update((value.len() as u64).to_le_bytes());
            fingerprint.update(value);
        }
        for bytes in [&before, &after] {
            fingerprint.update([u8::from(bytes.is_some())]);
            if let Some(bytes) = bytes {
                fingerprint.update(Sha256::digest(bytes));
            }
        }
        if let Some((source, digest)) = &source {
            fingerprint.update(source.to_string_lossy().as_bytes());
            fingerprint.update(digest.as_bytes());
        }
        if let Some(archive) = &archive {
            fingerprint.update(archive.to_string_lossy().as_bytes());
        }
        Ok(RecoveryPreview {
            root,
            file,
            before,
            after,
            source,
            archive,
            summary,
            token: format!("{:x}", fingerprint.finalize()),
        })
    }
}

#[cfg(test)]
mod tests;
