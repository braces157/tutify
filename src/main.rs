mod app;
mod auth;
mod catalog;
mod diagnostics;
mod model;
mod playback;
mod queue;
mod storage;
mod ui;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    version,
    about = "Standalone Spotify terminal player (Premium required)"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
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
    /// Stream one track with a minimal interface for first-stage audio validation.
    Probe { track: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    diagnostics::init();
    let cli = Cli::parse();
    let store = storage::Storage::local()?;
    let _instance = store.lock()?;
    match cli.command {
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
            println!("Logged out. Credentials and account queue removed.");
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
