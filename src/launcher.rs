//! Owns session restarts, account connections and dependency setup outside the TUI.
use crate::{
    Cli, app, auth,
    model::{MusicSource, SessionRequest},
    source,
    storage::Storage,
    terminal_profile, youtube,
};
use anyhow::Result;

pub(crate) async fn run(cli: &Cli, choice: source::Choice) -> Result<()> {
    let mut fresh = false;
    let mut next_notice = String::new();
    let mut use_free = false;
    loop {
        let mut selected = source::resolve(
            if use_free {
                source::Choice::Youtube
            } else {
                choice
            },
            fresh,
        )
        .await?;
        if !next_notice.is_empty() {
            selected.notice = std::mem::take(&mut next_notice);
        }
        let root = Storage::local()?;
        let mut store = match selected.source {
            MusicSource::Spotify | MusicSource::SpotifyVolta => root.clone(),
            MusicSource::Youtube => root.youtube()?,
            MusicSource::Volta => root.volta()?,
        };
        let mut instance = Some(store.lock()?);
        let automatic_audio = choice == source::Choice::Auto && !use_free;
        if selected.source == MusicSource::Spotify && !automatic_audio {
            let setup = async {
                if !selected.premium_verified {
                    auth::setup_catalog(&store).await?;
                    let plan = auth::TokenManager::load(&store.config()?)?
                        .account_plan()
                        .await?;
                    anyhow::ensure!(
                        plan != auth::AccountPlan::Free,
                        "Spotify account has no Premium subscription"
                    );
                }
                auth::setup_selected_spotify(&store).await
            }
            .await;
            if let Err(error) = setup {
                if choice != source::Choice::Auto {
                    return Err(error);
                }
                drop(instance.take());
                store = root.youtube()?;
                instance = Some(store.lock()?);
                selected.source = MusicSource::Youtube;
                selected.notice = format!(
                    "Spotify connection unavailable ({error}); free music is ready. F6 reconnects your account."
                );
            }
        }
        if selected.source == MusicSource::Youtube
            || (automatic_audio && crate::volta::playback::ffmpeg().is_err())
        {
            prepared_tools(
                youtube::prepare_music().await,
                automatic_audio,
                &mut selected.notice,
            )?;
        }
        if cli.glass_window && terminal_profile::available() {
            terminal_profile::prepare(&store.config()?)?;
            drop(instance.take());
            let launched = if choice == source::Choice::Auto {
                terminal_profile::launch_auto()?
            } else {
                match selected.source {
                    MusicSource::Spotify => terminal_profile::launch()?,
                    MusicSource::Youtube => terminal_profile::launch_youtube()?,
                    MusicSource::Volta => terminal_profile::launch_volta()?,
                    MusicSource::SpotifyVolta => terminal_profile::launch_auto()?,
                }
            };
            if launched {
                return Ok(());
            }
            instance = Some(store.lock()?);
        }
        let request = app::run_session(
            store,
            cli.native_glass,
            cli.glass,
            selected.source,
            &selected.notice,
            automatic_audio,
        )
        .await?;
        drop(instance);
        if request == SessionRequest::Quit {
            return Ok(());
        }
        if request == SessionRequest::UseFree {
            source::remember_free(&root).await;
            use_free = true;
            next_notice =
                "Spotify membership changed; free music is ready. Your Spotify queue is preserved."
                    .into();
            continue;
        }
        use_free = false;
        next_notice = connect(&root, request).await;
        fresh = true;
    }
}

fn prepared_tools(result: Result<()>, automatic: bool, notice: &mut String) -> Result<()> {
    if let Err(error) = result {
        if !automatic {
            return Err(error);
        }
        notice.push_str(&format!(" Playback tools unavailable ({error}); trying connected audio services. F6 retries setup."));
    }
    Ok(())
}

async fn connect(root: &Storage, request: SessionRequest) -> String {
    let result = async {
        match request {
            SessionRequest::ConnectSpotify => {
                let _lock = root.lock()?;
                auth::setup(root, None, true, false).await?;
            }
            SessionRequest::ConnectGoogle => {
                if youtube::music::Client::discover()?.is_none() {
                    youtube::music::setup().await?;
                }
                youtube::music_auth::login().await?;
            }
            SessionRequest::RepairPlayback => {
                youtube::setup().await?;
                youtube::music::setup().await?;
            }
            SessionRequest::Quit => (),
            SessionRequest::UseFree => (),
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    match result {
        Ok(()) => String::new(),
        Err(error) => {
            format!("Connection did not finish ({error}). Your queue is preserved; F6 retries.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_tool_setup_keeps_other_services_available_but_explicit_mode_reports_failure() {
        let mut notice = "Audio ready.".to_owned();
        prepared_tools(Err(anyhow::anyhow!("download failed")), true, &mut notice).unwrap();
        assert!(notice.contains("Audio ready.") && notice.contains("tools unavailable"));
        assert!(
            prepared_tools(
                Err(anyhow::anyhow!("download failed")),
                false,
                &mut String::new()
            )
            .is_err()
        );
    }
}
