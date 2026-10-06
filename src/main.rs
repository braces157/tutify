mod app;
mod auth;
mod build_info;
mod cache;
mod catalog;
mod demo;
mod diagnostics;
mod discord;
mod launcher;
mod library;
mod lyrics;
mod media_controls;
mod mix;
mod model;
mod playback;
mod providers;
mod queue;
mod service;
mod source;
pub mod stats;
mod storage;
mod terminal_profile;
mod ui;
pub mod visualizer;
mod youtube;

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Spotify Premium or free YouTube Music terminal player, with automatic source selection"
)]
struct Cli {
    /// Auto uses Spotify for saved Premium accounts, otherwise YouTube Music.
    #[arg(
        long = "source",
        value_enum,
        global = true,
        default_value = "auto",
        hide = true
    )]
    music_source: source::Choice,
    /// Internal marker used by the dedicated Windows Terminal Glass profile.
    #[arg(long, hide = true)]
    native_glass: bool,
    /// Run in the current terminal using the Glass background theme.
    #[arg(long, conflicts_with_all = ["glass_window", "native_glass"])]
    glass: bool,
    /// Open the full-resolution Glass background in a dedicated Windows Terminal window.
    #[arg(long, conflicts_with_all = ["glass", "native_glass"])]
    glass_window: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open the free YouTube player, or manage/test its optional tools.
    #[command(hide = true)]
    Youtube {
        #[command(subcommand)]
        command: Option<YoutubeCommand>,
    },
    /// Show immutable source/compiler identity and the observed executable hash.
    Version {
        /// Machine-readable build identity without authentication or saved-state access.
        #[arg(long)]
        json: bool,
    },
    /// Run the real terminal UI with an isolated fictional catalog and simulated playback.
    Demo,
    /// Read-only installation, terminal, state, credential, audio and catalog diagnostics.
    Doctor(diagnostics::doctor::Options),
    /// Preview redacted local diagnostics as JSON; optionally save a new report file.
    Support {
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Guided setup; reuse saved logins and open any missing browser login steps.
    Auth {
        /// Use a personal Spotify Developer app for catalog requests instead
        /// of the built-in shared PKCE client.
        #[arg(long)]
        client_id: Option<String>,
        #[arg(long)]
        streaming: bool,
        /// Replace saved logins, for example after authorization is revoked.
        #[arg(long)]
        force: bool,
    },
    /// Remove saved credentials and the account queue.
    Logout,
    /// Delete cached track names and availability; keep credentials and queue.
    ClearCache,
    /// Inspect, back up, or recover one state file without authentication or music.
    State {
        #[command(subcommand)]
        command: StateCommand,
    },
    /// Create a versioned backup of settings, queue, recipes, and aggregate stats.
    Backup {
        /// New backup file, outside the live data directory; never overwritten.
        file: PathBuf,
    },
    /// Preview a validated saved-state restore; credentials and cache stay unchanged.
    Restore {
        file: PathBuf,
        /// Apply exactly the preview identified by this confirmation token.
        #[arg(long, value_parser = storage::confirmation_token)]
        confirm: Option<String>,
    },
    /// Configure the Glass background image and dimming for both terminal orientations.
    Background {
        /// JPEG, PNG, WebP, or BMP image to use behind the terminal UI.
        #[arg(conflicts_with = "horizontal")]
        image: Option<PathBuf>,
        /// Vertical / portrait image to use when terminal is in portrait orientation.
        #[arg(long)]
        vertical: Option<PathBuf>,
        /// Horizontal / landscape image to use when terminal is in landscape orientation.
        #[arg(long, conflicts_with = "image")]
        horizontal: Option<PathBuf>,
        /// Darken the image for readable text (0 = bright, 85 = very dark).
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=85))]
        dim: Option<u8>,
    },
    /// Stream one track with a minimal interface for first-stage audio validation.
    Probe { track: String },
}

#[derive(Subcommand)]
enum YoutubeCommand {
    /// Install the isolated, pinned YouTube Music library adapter (Python 3.10+).
    MusicSetup,
    /// Connect Google in a dedicated browser; no Cloud project or password in Tuitify.
    Login,
    /// Forget the encrypted YouTube Music connection; keep your queue and Spotify login.
    Logout,
    /// List your connected YouTube Music playlists as JSON.
    Playlists,
    /// List your connected Liked Songs as JSON.
    Liked,
    /// Inspect an accessible YouTube Music playlist link as JSON.
    Playlist { link: String },
    /// Install/update checksum-verified yt-dlp and Deno; install FFmpeg if missing.
    Setup,
    /// Check the optional tool executables without signing in or playing music.
    Doctor,
    /// Search YouTube, or inspect a video link, as JSON without changing saved state.
    Search { query: String },
    /// Validate real audio decoding (muted by default) without changing saved state.
    Probe {
        track: String,
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u8).range(1..=60))]
        seconds: u8,
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=100))]
        volume: u8,
    },
}

#[derive(Subcommand)]
enum StateCommand {
    /// Report file/version/invariant failures; no saved state is changed.
    Inspect {
        #[arg(value_enum)]
        file: Option<storage::StateFile>,
    },
    /// Preserve this file's exact bytes, including damaged or future-version data.
    Backup {
        #[arg(value_enum)]
        file: storage::StateFile,
        destination: PathBuf,
    },
    /// Preview restoring just this file from a component or whole-state backup.
    Restore {
        #[arg(value_enum)]
        file: storage::StateFile,
        source: PathBuf,
        /// Optional destination for the original-file backup; defaults beside the data directory.
        #[arg(long)]
        backup: Option<PathBuf>,
        #[arg(long, value_parser = storage::confirmation_token)]
        confirm: Option<String>,
    },
    /// Preview resetting just this file; preserve its original in a component backup.
    Reset {
        #[arg(value_enum)]
        file: storage::StateFile,
        #[arg(long)]
        backup: Option<PathBuf>,
        #[arg(long, value_parser = storage::confirmation_token)]
        confirm: Option<String>,
    },
}

fn state_command(store: &storage::Storage, command: StateCommand) -> Result<()> {
    match command {
        StateCommand::Inspect { file } => {
            let files = file.map_or_else(|| storage::StateFile::ALL.to_vec(), |file| vec![file]);
            let mut healthy = true;
            for file in files {
                let inspection = store.inspect_state(file);
                healthy &= inspection.result.is_ok();
                println!("{inspection}");
            }
            ensure!(
                healthy,
                "Saved-state inspection found failures; original files were preserved"
            );
        }
        StateCommand::Backup { file, destination } => {
            store.backup_component(file, &destination)?;
            println!(
                "{} preserved in component backup: {}",
                file.name(),
                destination.display()
            );
            println!(
                "Exact original bytes are retained even if damaged or unsupported; restore requires valid supported state."
            );
        }
        StateCommand::Restore {
            file,
            source,
            backup,
            confirm,
        } => {
            recover_state(store, file, Some(source), backup, confirm)?;
        }
        StateCommand::Reset {
            file,
            backup,
            confirm,
        } => {
            recover_state(store, file, None, backup, confirm)?;
        }
    }
    Ok(())
}
fn recover_state(
    store: &storage::Storage,
    file: storage::StateFile,
    source: Option<PathBuf>,
    backup: Option<PathBuf>,
    confirm: Option<String>,
) -> Result<()> {
    let preview = store.recovery_preview(file, source.as_deref(), backup.as_deref())?;
    println!("{preview}");
    if let Some(confirm) = confirm {
        std::io::Write::flush(&mut std::io::stdout())?;
        preview.apply(store, &confirm)?;
        println!("Recovery complete for {}.", file.name());
    } else {
        println!("Preview only; no state files or credentials were changed.");
        let quote =
            |path: &std::path::Path| format!("'{}'", path.to_string_lossy().replace('\'', "''"));
        let action = match source {
            Some(source) => format!("restore {} {}", file.argument(), quote(&source)),
            None => format!("reset {}", file.argument()),
        };
        let backup = backup.map_or_else(String::new, |path| format!(" --backup {}", quote(&path)));
        println!(
            "Apply this preview: tuitify state {action}{backup} --confirm {}",
            preview.token
        );
    }
    Ok(())
}

#[tokio::main(worker_threads = 2)]
async fn main() -> Result<()> {
    diagnostics::init();
    let cli = Cli::parse();
    if let Some(Command::Version { json }) = cli.command {
        return build_info::run(json);
    }
    if matches!(cli.command, Some(Command::Demo)) {
        return app::run_demo(cli.glass).await;
    }
    if let Some(Command::Doctor(options)) = cli.command {
        return diagnostics::doctor::run(options).await;
    }
    if let Some(Command::Support { output }) = cli.command {
        return diagnostics::support::run(output).await;
    }
    if let Some(Command::Youtube {
        command: Some(command),
    }) = &cli.command
    {
        match command {
            YoutubeCommand::MusicSetup => return youtube::music::setup().await,
            YoutubeCommand::Login => {
                if youtube::music::Client::discover()?.is_none() {
                    youtube::music::setup().await?;
                }
                return youtube::music_auth::login().await;
            }
            YoutubeCommand::Logout => return youtube::music_auth::logout(),
            YoutubeCommand::Playlists | YoutubeCommand::Liked | YoutubeCommand::Playlist { .. } => {
                let music = youtube::music::Client::discover()?
                    .context("Run 'tuitify youtube music-setup' to install the library adapter")?;
                let browse = match command {
                    YoutubeCommand::Playlists => catalog::Browse::Playlists,
                    YoutubeCommand::Liked => catalog::Browse::Liked,
                    YoutubeCommand::Playlist { link } => catalog::Browse::Playlist(
                        youtube::music::playlist_id(link)
                            .context("Supply a YouTube playlist URL or youtube:playlist:ID")?,
                    ),
                    _ => unreachable!(),
                };
                let page = music.page(&browse, 0).await?;
                match page.rows {
                    catalog::Rows::Tracks(tracks) => println!("{}", serde_json::to_string_pretty(&tracks)?),
                    catalog::Rows::Playlists(playlists) => println!("{}", serde_json::to_string_pretty(&playlists.iter().map(|playlist| serde_json::json!({"id":playlist.id,"name":playlist.name,"owner":playlist.owner})).collect::<Vec<_>>())?),
                }
                return Ok(());
            }
            YoutubeCommand::Setup => return youtube::setup().await,
            YoutubeCommand::Doctor => return youtube::Tools::discover()?.doctor().await,
            YoutubeCommand::Search { query } => {
                let page = youtube::Tools::discover()?
                    .page(&catalog::Browse::Search(query.clone()), 0)
                    .await?;
                let catalog::Rows::Tracks(tracks) = page.rows else {
                    unreachable!()
                };
                println!("{}", serde_json::to_string_pretty(&tracks)?);
                return Ok(());
            }
            YoutubeCommand::Probe {
                track,
                seconds,
                volume,
            } => return youtube::probe(track, *seconds, *volume).await,
        }
    }
    if let Some(choice) = player_source(&cli)? {
        return launcher::run(&cli, choice).await;
    }
    let store = storage::Storage::local()?;
    let _instance = store.lock()?;
    match cli.command {
        Some(
            Command::Demo
            | Command::Doctor(_)
            | Command::Support { .. }
            | Command::Version { .. }
            | Command::Youtube { .. },
        ) => unreachable!(),
        Some(Command::Auth {
            client_id,
            streaming,
            force,
        }) => {
            auth::setup(&store, client_id, force, streaming).await?;
            println!("Setup complete. Run tuitify to open the player.");
            Ok(())
        }
        Some(Command::Logout) => {
            auth::delete_tokens()?;
            store.clear_queue()?;
            store.clear_cache()?;
            store.clear_stats()?;
            println!("Logged out. Credentials, account queue, and stats removed.");
            Ok(())
        }
        Some(Command::ClearCache) => {
            store.clear_cache()?;
            println!("Metadata cache cleared.");
            Ok(())
        }
        Some(Command::State { command }) => state_command(&store, command),
        Some(Command::Backup { file }) => {
            store.backup(&file)?;
            println!("Saved-state backup created: {}", file.display());
            println!(
                "Includes config, queue, mix recipes, and aggregate statistics. Credentials and cache are excluded."
            );
            Ok(())
        }
        Some(Command::Restore { file, confirm }) => {
            let preview = store.restore_preview(&file)?;
            println!("{preview}");
            if let Some(token) = confirm {
                std::io::Write::flush(&mut std::io::stdout())?;
                preview.apply(&store, &token)?;
                println!("Restore complete. Credentials and cache were left unchanged.");
            } else {
                println!("Preview only; no saved-state files were changed.");
                println!(
                    "Apply this preview: tuitify restore '{}' --confirm {}",
                    file.to_string_lossy().replace('\'', "''"),
                    preview.token
                );
            }
            Ok(())
        }
        Some(Command::Background {
            image,
            vertical,
            horizontal,
            dim,
        }) => {
            let mut config = store.config()?;
            config.theme = "glass".into();
            if let Some(dim_val) = dim {
                config.background_dim = dim_val;
            }
            let has_image_arg = horizontal.is_some() || image.is_some() || vertical.is_some();
            if has_image_arg {
                if let Some(path) = horizontal {
                    config.background_image = Some(
                        ui::background::validate_image(&path)?
                            .to_string_lossy()
                            .into_owned(),
                    );
                } else if let Some(path) = image {
                    config.background_image = Some(
                        ui::background::validate_image(&path)?
                            .to_string_lossy()
                            .into_owned(),
                    );
                    if vertical.is_none() {
                        config.background_image_vertical = None;
                    }
                }
                if let Some(path) = vertical {
                    config.background_image_vertical = Some(
                        ui::background::validate_image(&path)?
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
            } else if dim.is_none() {
                config.background_image = None;
                config.background_image_vertical = None;
                config.background_dim = 38;
            }
            let dim_val = config.background_dim;
            store.save_config(&config)?;
            if terminal_profile::available() {
                terminal_profile::prepare(&config)?;
            }
            match (&config.background_image, &config.background_image_vertical) {
                (Some(h), Some(v)) => {
                    println!(
                        "Glass background enabled: responsive horizontal ({h}) and vertical ({v}) (dim {dim_val}%)."
                    );
                }
                (Some(h), None) => {
                    println!("Glass background enabled: {h} (dim {dim_val}%).");
                }
                (None, Some(v)) => {
                    println!("Glass background enabled: vertical {v} (dim {dim_val}%).");
                }
                (None, None) => {
                    println!(
                        "Glass background enabled with the current Windows wallpaper (dim {dim_val}%)."
                    );
                }
            }
            Ok(())
        }
        Some(Command::Probe { track }) => {
            let id = model::track_id(&track)
                .or_else(|| model::valid_id(&track).then_some(track))
                .ok_or_else(|| {
                    anyhow::anyhow!("Supply a Spotify track link, URI, or 22-character ID")
                })?;
            let config = store.config()?;
            playback::probe(auth::TokenManager::load_streaming()?, config.client_id, id).await
        }
        None => unreachable!(),
    }
}

/// Management commands keep their existing storage and never trigger detection/setup.
fn player_source(cli: &Cli) -> Result<Option<source::Choice>> {
    match &cli.command {
        None => Ok(Some(cli.music_source)),
        Some(Command::Youtube { command: None }) => Ok(Some(source::Choice::Youtube)),
        _ => {
            ensure!(
                cli.music_source != source::Choice::Youtube,
                "Use 'tuitify youtube' for the YouTube player; Spotify commands require '--source spotify'"
            );
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_launch_defaults_to_auto_and_keeps_explicit_source_choices() {
        for (args, expected) in [
            (vec!["tuitify"], source::Choice::Auto),
            (vec!["tuitify", "--source", "auto"], source::Choice::Auto),
            (
                vec!["tuitify", "--source", "spotify"],
                source::Choice::Spotify,
            ),
            (
                vec!["tuitify", "--source", "youtube"],
                source::Choice::Youtube,
            ),
            (
                vec!["tuitify", "--glass", "youtube"],
                source::Choice::Youtube,
            ),
            (vec!["tuitify", "--glass-window"], source::Choice::Auto),
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert_eq!(player_source(&cli).unwrap(), Some(expected));
        }
        for args in [
            vec!["tuitify", "auth"],
            vec!["tuitify", "state", "inspect"],
            vec!["tuitify", "clear-cache"],
            vec!["tuitify", "background"],
        ] {
            assert!(
                player_source(&Cli::try_parse_from(args).unwrap())
                    .unwrap()
                    .is_none()
            );
        }
        let cli = Cli::try_parse_from(["tuitify", "--source", "youtube", "logout"]).unwrap();
        assert!(player_source(&cli).is_err());
    }

    #[test]
    fn youtube_source_and_commands_do_not_collide_with_recovery_source() {
        let cli = Cli::try_parse_from(["tuitify", "--source", "youtube"]).unwrap();
        assert_eq!(cli.music_source, source::Choice::Youtube);
        assert!(Cli::try_parse_from(["tuitify", "youtube"]).is_ok());
        assert!(Cli::try_parse_from(["tuitify", "youtube", "setup"]).is_ok());
        assert!(Cli::try_parse_from(["tuitify", "--glass", "youtube"]).is_ok());
        let cli = Cli::try_parse_from(["tuitify", "youtube", "probe", "dQw4w9WgXcQ"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Youtube {
                command: Some(YoutubeCommand::Probe { volume: 0, .. })
            })
        ));
        assert!(
            Cli::try_parse_from([
                "tuitify",
                "youtube",
                "probe",
                "dQw4w9WgXcQ",
                "--volume",
                "101"
            ])
            .is_err()
        );
        let cli =
            Cli::try_parse_from(["tuitify", "state", "restore", "queue", "saved.json"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::State {
                command: StateCommand::Restore { .. }
            })
        ));
    }

    #[test]
    fn cli_state_recovery_is_whitelisted_and_reset_restore_default_to_preview() {
        assert!(Cli::try_parse_from(["tuitify", "state", "inspect"]).is_ok());
        assert!(Cli::try_parse_from(["tuitify", "state", "inspect", "mix-recipes.json"]).is_ok());
        assert!(Cli::try_parse_from(["tuitify", "state", "backup", "recipes", "out.json"]).is_ok());
        for command in ["reset", "restore"] {
            let mut args = vec!["tuitify", "state", command, "recipes"];
            if command == "restore" {
                args.push("out.json");
            }
            let cli = Cli::try_parse_from(args.clone()).unwrap();
            assert!(matches!(
                cli.command,
                Some(Command::State {
                    command: StateCommand::Reset { confirm: None, .. }
                        | StateCommand::Restore { confirm: None, .. }
                })
            ));
            args.extend(["--confirm", "yes"]);
            assert!(Cli::try_parse_from(args).is_err());
        }
        for target in [
            "credentials",
            "../queue.json",
            "restore-journal.json",
            "other",
        ] {
            assert!(Cli::try_parse_from(["tuitify", "state", "reset", target]).is_err());
        }
    }

    #[test]
    fn cli_backup_and_restore_default_to_preview_and_require_a_valid_confirmation_token() {
        let cli = Cli::try_parse_from(["tuitify", "backup", "saved.json"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Backup { .. })));
        let cli = Cli::try_parse_from(["tuitify", "restore", "saved.json"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Restore { confirm: None, .. })
        ));
        let token = "A".repeat(64);
        let cli =
            Cli::try_parse_from(["tuitify", "restore", "saved.json", "--confirm", &token]).unwrap();
        let Some(Command::Restore {
            confirm: Some(parsed),
            ..
        }) = cli.command
        else {
            panic!("restore expected")
        };
        assert_eq!(parsed, "a".repeat(64));
        assert!(
            Cli::try_parse_from(["tuitify", "restore", "saved.json", "--confirm", "yes"]).is_err()
        );
        assert!(Cli::try_parse_from(["tuitify", "backup"]).is_err());
        assert!(Cli::try_parse_from(["tuitify", "restore"]).is_err());
    }

    #[test]
    fn cli_accepts_glass_flag() {
        let cli = Cli::try_parse_from(["tuitify", "--glass"]).unwrap();
        assert!(cli.glass);
        assert!(!cli.glass_window);
        assert!(!cli.native_glass);
    }

    #[test]
    fn cli_accepts_glass_window_flag() {
        let cli = Cli::try_parse_from(["tuitify", "--glass-window"]).unwrap();
        assert!(!cli.glass);
        assert!(cli.glass_window);
        assert!(!cli.native_glass);
    }

    #[test]
    fn cli_doctor_is_offline_by_default_and_network_is_explicit() {
        let cli = Cli::try_parse_from(["tuitify", "doctor", "--json"]).unwrap();
        match cli.command {
            Some(Command::Doctor(options)) => {
                assert!(!options.network);
                assert!(options.json);
                assert_eq!(options.timeout, 10);
                assert!(options.artist.is_none());
            }
            _ => panic!("expected doctor"),
        }
        for flag in ["--artist", "--playlist", "--seed-track"] {
            assert!(
                Cli::try_parse_from(["tuitify", "doctor", flag, "1111111111111111111111"]).is_err()
            );
            assert!(
                Cli::try_parse_from([
                    "tuitify",
                    "doctor",
                    "--network",
                    flag,
                    "1111111111111111111111"
                ])
                .is_ok()
            );
            assert!(
                Cli::try_parse_from(["tuitify", "doctor", "--network", flag, "bad/endpoint"])
                    .is_err()
            );
        }
        assert!(Cli::try_parse_from(["tuitify", "doctor", "--timeout", "2"]).is_err());
        for seconds in ["0", "31"] {
            assert!(
                Cli::try_parse_from(["tuitify", "doctor", "--network", "--timeout", seconds])
                    .is_err()
            );
        }
    }

    #[test]
    fn cli_support_defaults_to_preview_and_accepts_only_explicit_output() {
        let cli = Cli::try_parse_from(["tuitify", "support"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Support { output: None })
        ));
        let cli = Cli::try_parse_from([
            "tuitify",
            "support",
            "--output",
            "private folder/report.json",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Support { output }) => {
                assert_eq!(output, Some(PathBuf::from("private folder/report.json")))
            }
            _ => panic!("expected support"),
        }
        assert!(Cli::try_parse_from(["tuitify", "support", "--network"]).is_err());
    }

    #[test]
    fn cli_glass_conflicts_with_glass_window() {
        assert!(Cli::try_parse_from(["tuitify", "--glass", "--glass-window"]).is_err());
    }

    #[test]
    fn cli_glass_conflicts_with_native_glass() {
        assert!(Cli::try_parse_from(["tuitify", "--glass", "--native-glass"]).is_err());
    }

    #[test]
    fn cli_background_dim_options() {
        let cli = Cli::try_parse_from(["tuitify", "background", "--dim", "50"]).unwrap();
        match cli.command {
            Some(Command::Background { dim, .. }) => assert_eq!(dim, Some(50)),
            _ => panic!("expected Command::Background"),
        }
        let cli = Cli::try_parse_from(["tuitify", "background"]).unwrap();
        match cli.command {
            Some(Command::Background { dim, .. }) => assert_eq!(dim, None),
            _ => panic!("expected Command::Background"),
        }
    }

    #[test]
    fn cli_background_supports_two_orientations_and_rejects_ambiguous_images() {
        let cli = Cli::try_parse_from([
            "tuitify",
            "background",
            "--horizontal",
            "landscape.png",
            "--vertical",
            "portrait.png",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Background {
                image,
                horizontal,
                vertical,
                ..
            }) => {
                assert!(image.is_none());
                assert_eq!(horizontal.unwrap(), PathBuf::from("landscape.png"));
                assert_eq!(vertical.unwrap(), PathBuf::from("portrait.png"));
            }
            _ => panic!("expected Command::Background"),
        }

        assert!(
            Cli::try_parse_from([
                "tuitify",
                "background",
                "landscape.png",
                "--horizontal",
                "other.png",
            ])
            .is_err()
        );
    }
}
