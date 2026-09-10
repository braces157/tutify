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
mod ui;
pub mod visualizer;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    version,
    about = "Standalone Spotify terminal player, with an offline portfolio demo"
)]
struct Cli {
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
    /// Stream one track with a minimal interface for first-stage audio validation.
    Probe { track: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    diagnostics::init();
    let cli = Cli::parse();
    if matches!(cli.command, Some(Command::Demo)) {
        return app::run_demo().await;
    }
    let store = storage::Storage::local()?;
    let _instance = store.lock()?;
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
            app::run(store).await
        }
    }
}
