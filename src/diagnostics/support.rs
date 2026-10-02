//! Shareable reports are constructed from typed allowlisted data, never by
//! regex-cleaning status text, an upstream payload, or a doctor's raw output.
use super::{
    doctor,
    history::{ErrorRecord, History},
};
use crate::catalog::CapabilitySummary;
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CheckSummary {
    pub code: &'static str,
    pub result: doctor::Level,
}

#[derive(Clone, Debug, Serialize)]
struct Build {
    #[serde(flatten)]
    identity: crate::build_info::Identity,
    on_disk_executable_sha256: Option<String>,
    source_identity_embedded: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SupportReport {
    format: &'static str,
    version: u8,
    build: Build,
    observation_scope: &'static str,
    capabilities: Vec<CapabilitySummary>,
    checks: Vec<CheckSummary>,
    recent_errors: Vec<ErrorRecord>,
    privacy: &'static str,
}
impl SupportReport {
    pub fn new(
        history: &History,
        capabilities: Vec<CapabilitySummary>,
        checks: Vec<CheckSummary>,
    ) -> Self {
        let hash = std::env::current_exe()
            .ok()
            .and_then(|path| doctor::hash_file(&path).ok());
        Self {
            format: "tuitify-support",
            version: 1,
            build: Build {
                identity: crate::build_info::Identity::default(),
                on_disk_executable_sha256: hash,
                source_identity_embedded: true,
            },
            observation_scope: "Current client/account session only. Zero observations mean unknown, never a global availability claim. CLI reports cannot read another player's error history.",
            capabilities,
            checks,
            recent_errors: history.snapshot(),
            privacy: "Typed classifications only. Tokens, callback URLs, upstream payloads, account/resource IDs, paths, device names, searches, queue and song history are excluded. No report is uploaded automatically.",
        }
    }
    pub fn json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
    pub fn export(&self, path: &Path, data_root: Option<&Path>) -> Result<PathBuf> {
        let name = path
            .file_name()
            .context("Choose a support-report file name")?;
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent =
            fs::canonicalize(parent).context("Support-report parent directory must exist")?;
        let target = parent.join(name);
        if let Some(root) = data_root {
            // Resolve aliases/junctions. A missing data root is not created.
            let protected = if root.exists() {
                Some(fs::canonicalize(root)?)
            } else {
                root.parent()
                    .and_then(|parent| fs::canonicalize(parent).ok())
                    .zip(root.file_name())
                    .map(|(parent, name)| parent.join(name))
            };
            if let Some(root) = protected {
                let inside = if cfg!(windows) {
                    PathBuf::from(target.to_string_lossy().to_lowercase())
                        .starts_with(PathBuf::from(root.to_string_lossy().to_lowercase()))
                } else {
                    target.starts_with(root)
                };
                ensure!(
                    !inside,
                    "Keep support reports outside the live data directory"
                );
            }
        }
        let mut staged = tempfile::NamedTempFile::new_in(&parent)?;
        let json = self.json()?;
        ensure!(
            json.len() <= 128 * 1024,
            "Support report exceeds its size limit"
        );
        staged.write_all(json.as_bytes())?;
        staged.write_all(b"\n")?;
        staged.as_file().sync_all()?;
        staged
            .persist_noclobber(&target)
            .map_err(|error| error.error)
            .context("Support reports never overwrite existing files")?;
        Ok(target)
    }
    pub fn suggested_name() -> String {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!(
            "tuitify-support-{millis}-{:08x}.json",
            rand::random::<u32>()
        )
    }
}

pub(crate) async fn run(output: Option<PathBuf>) -> Result<()> {
    let collected = doctor::collect(doctor::Options::default()).await;
    let report = SupportReport::new(
        &History::default(),
        CapabilitySummary::unknown(),
        collected.safe_checks(),
    );
    if let Some(output) = output {
        let root = crate::storage::Storage::local_read_only()
            .ok()
            .map(|store| store.root);
        let target = report.export(&output, root.as_deref())?;
        println!(
            "Redacted support report saved: {}. Inspect it before sharing; nothing was uploaded.",
            target.display()
        );
    } else {
        println!("{}", report.json()?);
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct View {
    pub history: History,
    pub selected: usize,
    pub scroll: usize,
    pub preview: Option<SupportReport>,
    pub preview_json: String,
    pub exported_name: Option<String>,
    pub notice: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::history::Subsystem;
    #[test]
    fn support_export_never_turns_a_missing_data_root_into_a_report_file() {
        let temporary = tempfile::tempdir().unwrap();
        let missing = temporary.path().join("Tuitify");
        let report = SupportReport::new(&History::default(), CapabilitySummary::unknown(), vec![]);
        assert!(report.export(&missing, Some(&missing)).is_err());
        assert!(!missing.exists());
        assert!(
            report
                .export(&temporary.path().join("outside.json"), Some(&missing))
                .is_ok()
        );
        assert!(!missing.exists());
    }
    #[test]
    fn exported_fixture_omits_secrets_paths_and_history_and_never_clobbers() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("private-person-data");
        fs::create_dir(&root).unwrap();
        let private = "private-fixture-secret";
        let mut history = History::default();
        history.record(Subsystem::Catalog, &anyhow::anyhow!("{private} bearer eyJ.private-token http://127.0.0.1:8989/callback?code=private-code C:/Users/Private Person/Secret Song"));
        let report = SupportReport::new(
            &history,
            CapabilitySummary::unknown(),
            vec![CheckSummary {
                code: "state.queue",
                result: doctor::Level::Failure,
            }],
        );
        let preview = report.json().unwrap();
        let destination = temporary.path().join("share.json");
        report.export(&destination, Some(&root)).unwrap();
        let exported = fs::read_to_string(&destination).unwrap();
        assert_eq!(exported.trim_end(), preview);
        for secret in [
            private,
            "eyJ.private-token",
            "http://",
            "private-code",
            "Private Person",
            "Secret Song",
            "private-person-data",
        ] {
            assert!(!exported.contains(secret), "{secret}");
        }
        let parsed: serde_json::Value = serde_json::from_str(&exported).unwrap();
        assert_eq!(parsed["format"], "tuitify-support");
        assert_eq!(parsed["recent_errors"][0]["subsystem"], "catalog");
        assert_eq!(parsed["checks"][0]["result"], "failure");
        assert!(parsed["build"]["version"].is_string());
        assert_eq!(parsed["build"]["source_identity_embedded"], true);
        assert_eq!(parsed["build"]["source_sha256"].as_str().unwrap().len(), 64);
        assert_eq!(parsed["build"]["build_id"].as_str().unwrap().len(), 64);
        assert!(report.export(&destination, Some(&root)).is_err());
        assert_eq!(fs::read_to_string(&destination).unwrap(), exported);
        assert!(
            report
                .export(&root.join("config.json"), Some(&root))
                .is_err()
        );
        assert!(fs::read_dir(&root).unwrap().next().is_none());
    }
    #[test]
    fn report_snapshot_is_stable_after_later_errors_and_stays_bounded() {
        let mut history = History::default();
        history.record_text(Subsystem::Playback, "first failure");
        let report = SupportReport::new(&history, CapabilitySummary::unknown(), vec![]);
        let preview = report.json().unwrap();
        for _ in 0..500 {
            history.record_text(Subsystem::Storage, "private-path failure");
        }
        assert_eq!(report.json().unwrap(), preview);
        assert!(
            SupportReport::new(&history, CapabilitySummary::unknown(), vec![])
                .json()
                .unwrap()
                .len()
                < 128 * 1024
        );
    }
}
