mod app;
mod auth;
mod cache;
mod catalog;
mod demo;
mod diagnostics;
mod discord;
mod library;
mod lyrics;
mod media_controls;
mod mix;
mod model;
mod playback;
mod queue;
pub mod stats;
mod storage;
mod terminal_profile;
mod ui;
pub mod visualizer;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Standalone Spotify terminal player, with an offline portfolio demo"
)]
struct Cli {
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
    /// Run the real terminal UI with an isolated fictional catalog and simulated playback.
    Demo,
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
    /// Use a dimmed image behind the UI. Without IMAGE, use the current Windows wallpaper.
    Background {
        /// JPEG, PNG, WebP, or BMP image to use behind the terminal UI.
        image: Option<PathBuf>,
        /// Darken the image for readable text (0 = bright, 85 = very dark).
        #[arg(long, default_value_t = 38, value_parser = clap::value_parser!(u8).range(0..=85))]
        dim: u8,
    },
    /// Stream one track with a minimal interface for first-stage audio validation.
    Probe { track: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    diagnostics::init();
    let cli = Cli::parse();
    if matches!(cli.command, Some(Command::Demo)) {
        return app::run_demo(cli.glass).await;
    }
    let store = storage::Storage::local()?;
    let mut instance = Some(store.lock()?);
    if cli.command.is_none() && cli.glass_window {
        let config = store.config()?;
        if terminal_profile::available() {
            terminal_profile::prepare(&config)?;
            drop(instance.take());
            if terminal_profile::launch()? {
                return Ok(());
            }
            instance = Some(store.lock()?);
        }
    }
    let _instance = instance;
    match cli.command {
        Some(Command::Demo) => unreachable!(),
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
        Some(Command::Background { image, dim }) => {
            let mut config = store.config()?;
            config.theme = "glass".into();
            config.background_dim = dim;
            config.background_image = match image {
                Some(path) => Some(
                    ui::background::validate_image(&path)?
                        .to_string_lossy()
                        .into_owned(),
                ),
                None => None,
            };
            store.save_config(&config)?;
            if let Some(path) = &config.background_image {
                println!("Glass background enabled: {path} (dim {dim}%).");
            } else {
                println!(
                    "Glass background enabled with the current Windows wallpaper (dim {dim}%)."
                );
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
        None => {
            auth::setup(&store, None, false, false).await?;
            app::run(store, cli.native_glass, cli.glass).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn cli_glass_conflicts_with_glass_window() {
        assert!(Cli::try_parse_from(["tuitify", "--glass", "--glass-window"]).is_err());
    }

    #[test]
    fn cli_glass_conflicts_with_native_glass() {
        assert!(Cli::try_parse_from(["tuitify", "--glass", "--native-glass"]).is_err());
    }
}
