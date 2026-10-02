//! Immutable compiled identity and explicitly separate observation of disk bytes.
use anyhow::Result;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Identity {
    pub version: &'static str,
    pub source_commit: Option<&'static str>,
    pub source_dirty: Option<bool>,
    pub source_sha256: &'static str,
    pub build_id: &'static str,
    pub target: &'static str,
    pub architecture: &'static str,
    pub os: &'static str,
    pub profile: &'static str,
    pub rustc: &'static str,
    pub settings_sha256: &'static str,
}
impl Default for Identity {
    fn default() -> Self {
        let commit = env!("TUITIFY_BUILD_SOURCE_COMMIT");
        Self {
            version: env!("CARGO_PKG_VERSION"),
            source_commit: (!commit.is_empty()).then_some(commit),
            source_dirty: match env!("TUITIFY_BUILD_SOURCE_DIRTY") {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            source_sha256: env!("TUITIFY_BUILD_SOURCE_SHA256"),
            build_id: env!("TUITIFY_BUILD_ID"),
            target: env!("TUITIFY_BUILD_TARGET"),
            architecture: std::env::consts::ARCH,
            os: std::env::consts::OS,
            profile: env!("TUITIFY_BUILD_PROFILE"),
            rustc: env!("TUITIFY_BUILD_RUSTC"),
            settings_sha256: env!("TUITIFY_BUILD_SETTINGS_SHA256"),
        }
    }
}
impl Identity {
    pub fn detail(&self) -> String {
        format!(
            "tuitify {} · {} · {} · {}\nBuild ID: {}\nSource SHA-256: {}\nSource commit: {} · source dirty: {}\nCompiler: {} · settings SHA-256: {}",
            self.version,
            self.target,
            self.profile,
            self.architecture,
            self.build_id,
            self.source_sha256,
            self.source_commit.unwrap_or("unavailable"),
            self.source_dirty
                .map_or("unknown", |dirty| if dirty { "yes" } else { "no" }),
            self.rustc,
            self.settings_sha256
        )
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Report {
    pub format: &'static str,
    pub format_version: u8,
    pub build: Identity,
    pub on_disk_executable_sha256: Option<String>,
}
impl Default for Report {
    fn default() -> Self {
        Self {
            format: "tuitify-build",
            format_version: 1,
            build: Identity::default(),
            on_disk_executable_sha256: std::env::current_exe()
                .ok()
                .and_then(|path| crate::diagnostics::doctor::hash_file(&path).ok()),
        }
    }
}
pub(crate) fn run(json: bool) -> Result<()> {
    let report = Report::default();
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{}", report.build.detail());
        println!(
            "On-disk executable SHA-256: {}",
            report
                .on_disk_executable_sha256
                .as_deref()
                .unwrap_or("unavailable")
        );
        println!(
            "Compiled identity describes this process; the disk hash describes the file currently at its launch path."
        );
    }
    Ok(())
}
