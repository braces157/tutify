//! Read-only command: deliberately runs before Storage::local/lock and auth setup.
use crate::{
    auth,
    catalog::Catalog,
    service::{FailureKind, ServiceFailure},
    storage::{StateFile, Storage},
};
use anyhow::{Result, ensure};
use clap::Args;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{IsTerminal, Read},
    path::Path,
    time::Duration,
};

mod installation;
#[cfg(test)]
mod tests;

#[derive(Args, Default)]
pub(crate) struct Options {
    /// Opt into bounded Spotify GET probes. Never log in or refresh tokens.
    #[arg(long)]
    pub network: bool,
    /// Print structured local diagnostics (includes local installation/data paths).
    #[arg(long)]
    pub json: bool,
    /// Probe this artist's top-tracks capability for the saved catalog account.
    #[arg(long, requires = "network", value_parser = spotify_id)]
    pub artist: Option<String>,
    /// Probe this playlist's contents; no playlist or library changes are made.
    #[arg(long, requires = "network", value_parser = spotify_id)]
    pub playlist: Option<String>,
    /// Probe recommendations for this seed track, without discovery fallback.
    #[arg(long, requires = "network", value_parser = spotify_id)]
    pub seed_track: Option<String>,
    /// Maximum seconds per network probe, including response body (1-30).
    #[arg(long, requires = "network", default_value = "10", value_parser = clap::value_parser!(u64).range(1..=30))]
    pub timeout: u64,
}

fn spotify_id(value: &str) -> std::result::Result<String, String> {
    crate::model::valid_id(value)
        .then(|| value.to_owned())
        .ok_or_else(|| "Use a 22-character Spotify ID containing only letters and digits".into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Level {
    Pass,
    Warning,
    Failure,
    Unknown,
}

#[derive(Debug, Serialize)]
struct Check {
    name: String,
    level: Level,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<String>,
}

#[derive(Default, Serialize)]
pub(crate) struct Report {
    build: crate::build_info::Report,
    checks: Vec<Check>,
}
impl Report {
    pub fn safe_checks(&self) -> Vec<super::support::CheckSummary> {
        self.checks
            .iter()
            .filter_map(|check| {
                // Closed allowlist. Never copy detail/action text, paths, device
                // names, environment values or arbitrary future check names.
                let code = match check.name.as_str() {
                    "build" => "build",
                    "terminal" => "terminal",
                    "data_directory" => "data_directory",
                    "installation.running" => "installation.running",
                    "installation.process_path" => "installation.process_path",
                    "installation.current_directory" => "installation.current_directory",
                    "installation.shell_lookup" => "installation.shell_lookup",
                    "installation.canonical" => "installation.canonical",
                    "installation.saved_path" => "installation.saved_path",
                    "installation.user_path_order" => "installation.user_path_order",
                    "state.config" => "state.config",
                    "state.queue" => "state.queue",
                    "state.recipes" => "state.recipes",
                    "state.stats" => "state.stats",
                    "state.cache" => "state.cache",
                    "state.restore_journal" => "state.restore_journal",
                    "state.snapshot" => "state.snapshot",
                    "credentials.catalog" => "credentials.catalog",
                    "credentials.streaming" => "credentials.streaming",
                    "credentials.account_mapping" => "credentials.account_mapping",
                    "audio.devices" => "audio.devices",
                    "audio.default" => "audio.default",
                    "catalog.profile" => "catalog.profile",
                    "catalog.liked" => "catalog.liked",
                    "catalog.playlists" => "catalog.playlists",
                    "catalog.artist_top_tracks" => "catalog.artist_top_tracks",
                    "catalog.playlist_items" => "catalog.playlist_items",
                    "catalog.recommendations" => "catalog.recommendations",
                    "catalog.library_writes" => "catalog.library_writes",
                    "catalog.network" => "catalog.network",
                    "streaming.playback" => "streaming.playback",
                    _ => return None,
                };
                Some(super::support::CheckSummary {
                    code,
                    result: check.level,
                })
            })
            .collect()
    }
    fn add(
        &mut self,
        name: impl Into<String>,
        level: Level,
        detail: impl Into<String>,
        action: Option<&str>,
    ) {
        self.checks.push(Check {
            name: name.into(),
            level,
            detail: printable(&detail.into()),
            action: action.map(printable),
        });
    }
    fn healthy(&self) -> bool {
        self.checks
            .iter()
            .all(|check| check.level != Level::Failure)
    }
    fn print(&self, json: bool) -> Result<()> {
        if json {
            println!("{}", serde_json::to_string_pretty(self)?);
        } else {
            println!(
                "Tuitify doctor — read-only; no music, settings, credentials or journals changed"
            );
            for check in &self.checks {
                println!("[{:?}] {}: {}", check.level, check.name, check.detail);
                if let Some(action) = &check.action {
                    println!("  Next: {action}");
                }
            }
            println!(
                "{}",
                if self.healthy() {
                    "No failed checks. Unknown checks still need verification."
                } else {
                    "Failed checks found; follow the recovery actions above."
                }
            );
        }
        Ok(())
    }
}

// Devices, environment and file names can contain terminal controls. Never let
// them inject ANSI/OSC commands into the diagnostic terminal.
fn printable(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .take(2048)
        .collect()
}

pub(crate) fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    if file.metadata()?.len() > 512 * 1024 * 1024 {
        return Err(std::io::Error::other(
            "Executable exceeds diagnostic size bound",
        ));
    }
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut remaining: usize = 512 * 1024 * 1024;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        remaining = remaining
            .checked_sub(read)
            .ok_or_else(|| std::io::Error::other("Executable grew beyond diagnostic bound"))?;
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn terminal_check(
    report: &mut Report,
    input: bool,
    output: bool,
    size: std::io::Result<(u16, u16)>,
) {
    if !input || !output {
        report.add("terminal", Level::Warning, "Input or output is redirected. Doctor works here; the interactive player needs a terminal.", Some("Open Windows Terminal or a terminal console and run tuitify there."));
    } else {
        match size {
            Ok((width, height)) if width >= 32 && height >= 10 => report.add("terminal", Level::Pass, format!("Interactive terminal {width}x{height}; meets the UI minimum. ANSI/color support still depends on the terminal."), None),
            Ok((width, height)) => report.add("terminal", Level::Failure, format!("Terminal {width}x{height} is below the 32x10 UI minimum."), Some("Resize the terminal to at least 32 columns and 10 rows.")),
            Err(_) => report.add("terminal", Level::Failure, "Cannot query terminal dimensions.", Some("Run in a Windows Terminal console; check console permissions and retry.")),
        }
    }
}

fn state_checks(report: &mut Report, store: &Storage) -> bool {
    match fs::symlink_metadata(&store.root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            report.add(
                "data_directory",
                Level::Warning,
                format!(
                    "{} does not exist; doctor did not create it.",
                    store.root.display()
                ),
                Some("Run tuitify auth when ready for first setup."),
            );
        }
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            if fs::read_dir(&store.root).is_err() {
                report.add("data_directory", Level::Failure, "Cannot read the data directory; no files were changed.", Some("Check your account's access to LOCALAPPDATA/Tuitify and preserve its files before recovery."));
                return false;
            }
            report.add("data_directory", Level::Pass, format!("{} is readable. Write permissions and free space were not tested by creating files.", store.root.display()), None);
        }
        _ => {
            report.add("data_directory", Level::Failure, "Data root is inaccessible, a link, or not a directory; doctor did not traverse it.", Some("Check LOCALAPPDATA/Tuitify and preserve the existing data before changing its location or permissions."));
            return false;
        }
    }
    for file in StateFile::ALL {
        let inspection = store.inspect_state(file);
        report.add(format!("state.{}", file.argument()), if inspection.result.is_ok() { Level::Pass } else { Level::Failure }, inspection.to_string(),
            inspection.result.is_err().then_some("Run tuitify state inspect, then back up the affected file with tuitify state backup COMPONENT FILE. Preview targeted restore or reset before applying it."));
    }
    match fs::symlink_metadata(store.root.join("restore-journal.json")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => report.add("state.restore_journal", Level::Pass, "No interrupted-restore journal found.", None),
        Ok(_) => report.add("state.restore_journal", Level::Failure, "A restore journal exists. Doctor did not validate, recover or remove it; the state snapshot may be incomplete.", Some("Quit the player and preserve the entire data directory, including restore-journal.json. Normal Tuitify startup attempts validated recovery; if it fails, retain the journal and reported failure for support.")),
        Err(_) => report.add("state.restore_journal", Level::Failure, "Cannot inspect the restore journal.", Some("Check data-directory permissions and preserve all files before recovery.")),
    }
    report.add("state.snapshot", Level::Unknown, "Read-only snapshot without an instance lock; files can change if another player is running.", Some("Quit Tuitify and rerun doctor for a stable saved-state snapshot."));
    store.config().is_ok()
}

fn credential_check(report: &mut Report, name: &str, state: auth::doctor::State) {
    use auth::doctor::State;
    let (level, detail, action) = match state {
        State::Present => (
            Level::Pass,
            "Saved credential is present, structurally valid and unexpired. Remote acceptance was not checked.",
            None,
        ),
        State::Expired => (
            Level::Warning,
            "Saved credential is present but its access token has expired or expires within 60 seconds. Doctor does not refresh it.",
            Some(
                "Normal player startup can refresh the saved login. Afterward rerun doctor; if the login was revoked, use tuitify auth --force (or --streaming --force for streaming). Avoid repeated logins during rate or quota limits.",
            ),
        ),
        State::Missing => (
            Level::Warning,
            "No reusable saved credential found.",
            Some(if name.ends_with("streaming") {
                "Run tuitify auth --streaming to set up streaming access."
            } else {
                "Run tuitify auth to set up catalog access."
            }),
        ),
        State::Invalid => (
            Level::Failure,
            "Saved credential is damaged or unsupported; it was preserved.",
            Some(
                "Preserve local state with tuitify backup FILE. Review account recovery before deliberately replacing the saved login with tuitify auth --force.",
            ),
        ),
        State::Unavailable => (
            Level::Failure,
            "Windows Credential Manager could not be read; upstream errors and secrets are omitted.",
            Some(
                "Run under the Windows account that saved the login; check Credential Manager access before changing credentials.",
            ),
        ),
    };
    report.add(name, level, detail, action);
}

fn audio_checks(report: &mut Report) {
    let host = rodio::cpal::default_host();
    match host.output_devices() {
        Ok(devices) => {
            let mut count = 0;
            for device in devices.take(128) {
                count += 1;
                match device.name() {
                    Ok(name) => {
                        report.add(format!("audio.device.{count}"), Level::Pass, name, None)
                    }
                    Err(_) => report.add(
                        format!("audio.device.{count}"),
                        Level::Warning,
                        "Device exists but its name is unavailable.",
                        Some("Check Windows Sound settings and the audio driver."),
                    ),
                }
            }
            report.add("audio.devices", if count == 0 { Level::Failure } else { Level::Pass }, format!("{count} output devices enumerated (maximum 128); no audio stream opened."), (count == 0).then_some("Connect or enable an output device in Windows Sound settings and check its driver."));
        }
        Err(_) => report.add(
            "audio.devices",
            Level::Failure,
            "Could not enumerate output devices.",
            Some("Check Windows Audio service and output-device drivers, then retry."),
        ),
    }
    match host.default_output_device() {
        Some(device) => match device.default_output_config() {
            Ok(config) => report.add("audio.default", Level::Pass, format!("Default output reports {} Hz, {} channels, {:?}. This proves configuration access, not audible playback.", config.sample_rate().0, config.channels(), config.sample_format()), None),
            Err(_) => report.add("audio.default", Level::Failure, "Default device exists but its output configuration is unavailable.", Some("Choose a working Windows default output device and check its driver.")),
        },
        None => report.add("audio.default", Level::Failure, "Windows has no default output device.", Some("Select a default output device in Windows Sound settings.")),
    }
}

struct Probe {
    name: &'static str,
    path: String,
    query: Vec<(&'static str, String)>,
    profile: bool,
    tracks: bool,
}
fn probes(options: &Options) -> Vec<Probe> {
    let mut probes = vec![
        Probe {
            name: "catalog.profile",
            path: "/me".into(),
            query: vec![],
            profile: true,
            tracks: false,
        },
        Probe {
            name: "catalog.liked",
            path: "/me/tracks".into(),
            query: vec![("limit", "1".into())],
            profile: false,
            tracks: false,
        },
        Probe {
            name: "catalog.playlists",
            path: "/me/playlists".into(),
            query: vec![("limit", "1".into())],
            profile: false,
            tracks: false,
        },
    ];
    if let Some(id) = &options.artist {
        probes.push(Probe {
            name: "catalog.artist_top_tracks",
            path: format!("/artists/{id}/top-tracks"),
            query: vec![],
            profile: false,
            tracks: true,
        });
    }
    if let Some(id) = &options.playlist {
        probes.push(Probe {
            name: "catalog.playlist_items",
            path: format!("/playlists/{id}/items"),
            query: vec![("limit", "1".into())],
            profile: false,
            tracks: false,
        });
    }
    if let Some(id) = &options.seed_track {
        probes.push(Probe {
            name: "catalog.recommendations",
            path: "/recommendations".into(),
            query: vec![("seed_tracks", id.clone()), ("limit", "1".into())],
            profile: false,
            tracks: true,
        });
    }
    probes
}

async fn network_checks(report: &mut Report, catalog: &Catalog, options: &Options) {
    let mut stopped = false;
    for probe in probes(options) {
        if stopped {
            report.add(
                probe.name,
                Level::Unknown,
                "Skipped after a systemic failure to avoid repeated requests.",
                Some("Resolve the preceding service failure, then retry doctor --network."),
            );
            continue;
        }
        let result = tokio::time::timeout(
            Duration::from_secs(options.timeout),
            catalog.get(&probe.path, &probe.query),
        )
        .await;
        match result {
            Err(_) => {
                stopped = true;
                report.add(probe.name, Level::Failure, format!("Probe exceeded its {} second limit; pending request cancelled.", options.timeout), Some("Check your connection or retry later with doctor --network; increase --timeout up to 30 if necessary."));
            }
            Ok(Ok(value)) => {
                let shape = if probe.profile {
                    ["account_id", "id"]
                        .iter()
                        .any(|name| value[name].as_str().is_some_and(|id| !id.trim().is_empty()))
                } else {
                    value[if probe.tracks { "tracks" } else { "items" }].is_array()
                };
                if shape {
                    report.add(probe.name, Level::Pass, "GET accepted with the expected response shape for this account and requested resource. No song titles, account IDs or payload retained.", None);
                } else {
                    stopped = true;
                    report.add(
                        probe.name,
                        Level::Failure,
                        "GET returned an unexpected response shape; raw payload omitted.",
                        Some("Retry later or update Tuitify if the provider schema changed."),
                    );
                }
            }
            Ok(Err(error)) => {
                if let Some(failure) = error.downcast_ref::<ServiceFailure>() {
                    stopped = !matches!(
                        failure.kind,
                        FailureKind::AccessRestricted | FailureKind::MissingItem
                    );
                    report.add(probe.name, Level::Failure, failure.to_string(), Some("Follow this classified recovery action, then rerun doctor --network. Endpoint denial describes this account/resource only."));
                } else {
                    stopped = true;
                    report.add(
                        probe.name,
                        Level::Failure,
                        "Catalog probe failed; unclassified upstream details omitted.",
                        Some("Review saved login/state checks, check connectivity, then retry."),
                    );
                }
            }
        }
    }
}

fn unknown_capabilities(report: &mut Report, options: &Options) {
    if !options.network {
        for probe in probes(&Options::default()) {
            report.add(probe.name, Level::Unknown, "Offline: this catalog capability has not been probed.", Some("Run doctor --network to check this capability using an existing unexpired catalog token."));
        }
    }
    for (name, selected, action) in [
        (
            "catalog.artist_top_tracks",
            options.artist.is_some(),
            "Use doctor --network --artist ID to check one artist; no global support is inferred.",
        ),
        (
            "catalog.playlist_items",
            options.playlist.is_some(),
            "Use doctor --network --playlist ID to check one playlist's content access.",
        ),
        (
            "catalog.recommendations",
            options.seed_track.is_some(),
            "Use doctor --network --seed-track ID to check one seed without fallback.",
        ),
    ] {
        if !options.network || !selected {
            report.add(name, Level::Unknown, "No observation in this diagnostic session; another player's session observations are not shared.", Some(action));
        }
    }
    report.add(
        "catalog.library_writes",
        Level::Unknown,
        "Save/remove capabilities are not probed. Doctor performs no library mutations.",
        None,
    );
    report.add("streaming.playback", Level::Unknown, "No streaming handshake or audio stream started; credential presence and device enumeration do not prove playback works.", Some("After resolving failures, use a fresh normal playback session to verify sound on your hardware."));
}

pub(crate) async fn collect(options: Options) -> Report {
    let mut report = Report::default();
    report.add("build", Level::Pass, report.build.build.detail(), None);
    installation::checks(&mut report);
    terminal_check(
        &mut report,
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
        crossterm::terminal::size(),
    );
    let store = Storage::local_read_only();
    let config = match &store {
        Ok(store) if state_checks(&mut report, store) => store.config().ok(),
        Ok(_) => None,
        Err(_) => {
            report.add(
                "data_directory",
                Level::Failure,
                "LOCALAPPDATA is not set; no data directory was created.",
                Some("Run under a Windows user account with LOCALAPPDATA configured."),
            );
            None
        }
    };
    let credentials = auth::doctor::Credentials::read();
    credential_check(&mut report, "credentials.catalog", credentials.catalog);
    credential_check(&mut report, "credentials.streaming", credentials.streaming);
    match credentials.same_account {
        Some(true) => report.add("credentials.account_mapping", Level::Pass, "Saved account namespaces are compatible; fresh remote identity verification remains part of playback startup.", None),
        Some(false) => report.add("credentials.account_mapping", Level::Failure, "Saved streaming/catalog identity mapping is mismatched or ambiguous; credentials preserved.", Some("Authenticate the same account with tuitify auth --streaming --force. To deliberately switch accounts, first back up state and review the logout flow.")),
        None => report.add("credentials.account_mapping", Level::Unknown, "Both valid saved credentials are required for a local account comparison.", Some("Resolve credential checks first; doctor does not authenticate or infer an account from one token.")),
    }
    audio_checks(&mut report);
    unknown_capabilities(&mut report, &options);
    if options.network {
        match config
            .and_then(|config| credentials.catalog_manager(&config).ok())
            .and_then(|tokens| Catalog::new(tokens).ok())
        {
            Some(catalog) => network_checks(&mut report, &catalog, &options).await,
            None => {
                report.add("catalog.network", Level::Failure, "Network probes skipped: valid configuration and an unexpired saved catalog token are required. No refresh attempted.", Some("Resolve configuration/credential checks. Normal player startup can refresh expired logins; revoked logins require deliberate authentication. Then retry doctor --network."));
                for probe in probes(&options) {
                    report.add(probe.name, Level::Unknown, "Probe skipped because configuration or an unexpired saved catalog token is unavailable.", Some("Resolve the preceding configuration/credential failure, then retry doctor --network."));
                }
            }
        }
    } else {
        report.add("catalog.network", Level::Unknown, "Offline by default: no Spotify, OAuth, similarity, or streaming requests made.", Some("Run tuitify doctor --network to opt into bounded GET checks using an existing unexpired catalog token."));
    }
    report
}

pub(crate) async fn run(options: Options) -> Result<()> {
    let json = options.json;
    let report = collect(options).await;
    report.print(json)?;
    ensure!(
        report.healthy(),
        "Doctor found failed checks; no settings or saved credentials were changed"
    );
    Ok(())
}
