//! A session owns a matching catalog and playback backend; the UI uses one protocol.
use crate::{
    auth::TokenManager, catalog::Catalog, model::MusicSource, playback::Playback, storage::Config,
    visualizer::AudioVisualizer, youtube,
};
use anyhow::Result;
use std::sync::Arc;
pub(crate) struct Services {
    pub catalog: Catalog,
    pub playback: Playback,
    pub music_catalog: bool,
    pub library_connected: bool,
}
pub(crate) fn open(
    source: MusicSource,
    config: &Config,
    visualizer: Arc<AudioVisualizer>,
) -> Result<Services> {
    if config.automatic_audio {
        return open_automatic(source, config, visualizer);
    }
    let mut audio_catalog = None;
    let mut services = match source {
        MusicSource::Spotify => Ok::<Services, anyhow::Error>(Services {
            catalog: Catalog::new(TokenManager::load(config)?)?,
            playback: Playback::spawn_with_visualizer(
                TokenManager::load_streaming()?,
                config.client_id.clone(),
                config.volume,
                visualizer.clone(),
            ),
            music_catalog: false,
            library_connected: true,
        }),
        MusicSource::Youtube => {
            let tools = youtube::Tools::discover()?;
            let music_catalog = tools.music.is_some();
            let library_connected = tools.music.as_ref().is_some_and(|music| music.connected());
            Ok(Services {
                catalog: Catalog::youtube(tools.clone())?,
                playback: Playback::spawn_youtube(tools, config.volume, visualizer.clone()),
                music_catalog,
                library_connected,
            })
        }
        MusicSource::Volta => {
            let client = crate::volta::Client::load()?;
            Ok(Services {
                catalog: Catalog::volta(client.clone())?,
                playback: Playback::spawn_volta(client, config.volume, visualizer.clone())?,
                music_catalog: false,
                library_connected: true,
            })
        }
        MusicSource::SpotifyVolta => {
            let catalog = Catalog::new(TokenManager::load(config)?)?;
            let client = crate::volta::Client::load()?;
            audio_catalog = Some(Catalog::volta(client.clone())?);
            Ok(Services {
                playback: Playback::spawn_spotify_volta(
                    client,
                    catalog.clone(),
                    config.volume,
                    visualizer.clone(),
                )?,
                catalog,
                music_catalog: false,
                library_connected: true,
            })
        }
    }?;
    let deezer = crate::deezer::Client::new()?;
    services.catalog = services.catalog.with_deezer(deezer.clone());
    let resolver = audio_catalog
        .unwrap_or_else(|| services.catalog.clone())
        .with_deezer(deezer.clone());
    services.playback = services.playback.with_catalog(resolver);
    if matches!(source, MusicSource::Volta | MusicSource::SpotifyVolta) {
        // Each retry resolves the original Deezer recording independently. A
        // translated Volta ID must not make another provider depend on Volta.
        if let Ok(tools) = youtube::Tools::discover() {
            let catalog = Catalog::youtube(tools.clone())?.with_deezer(deezer.clone());
            let fallback = Playback::spawn_youtube(tools, config.volume, visualizer.clone())
                .with_catalog(catalog);
            services.playback = services.playback.with_deezer_fallback(fallback, "YouTube");
        }
        // Standalone Volta settings do not contain Spotify's catalog client ID.
        let spotify_config = if source == MusicSource::Volta {
            crate::storage::Storage::local_read_only()
                .and_then(|store| store.config())
                .ok()
        } else {
            Some(config.clone())
        };
        if let Ok(Some(tokens)) = TokenManager::load_optional_streaming()
            && let Some(spotify_config) = spotify_config
            && let Ok(Some(catalog_tokens)) = TokenManager::load_optional(&spotify_config)
        {
            let catalog = Catalog::new(catalog_tokens)?.with_deezer(deezer);
            let fallback = Playback::spawn_with_visualizer(
                tokens,
                crate::auth::STREAMING_CLIENT_ID.into(),
                config.volume,
                visualizer,
            )
            .with_catalog(catalog);
            services.playback = services.playback.with_deezer_fallback(fallback, "Spotify");
        }
    }
    services.playback = services.playback.with_recovery(services.catalog.clone());
    Ok(services)
}

fn open_automatic(
    source: MusicSource,
    config: &Config,
    visualizer: Arc<AudioVisualizer>,
) -> Result<Services> {
    let deezer = crate::deezer::Client::new()?;
    let spotify_config = if matches!(source, MusicSource::Volta | MusicSource::Youtube) {
        crate::storage::Storage::local_read_only()
            .and_then(|store| store.config())
            .ok()
    } else {
        Some(config.clone())
    };
    let spotify_catalog = spotify_config
        .as_ref()
        .and_then(|config| TokenManager::load_optional(config).ok().flatten())
        .map(Catalog::new)
        .transpose()?
        .map(|catalog| catalog.with_deezer(deezer.clone()));
    let volta = crate::volta::Client::load().ok();
    let volta_catalog = volta
        .as_ref()
        .map(|client| Catalog::volta(client.clone()))
        .transpose()?
        .map(|catalog| catalog.with_deezer(deezer.clone()));
    let tools = youtube::Tools::discover().ok();
    let youtube_catalog = tools
        .as_ref()
        .map(|tools| Catalog::youtube(tools.clone()))
        .transpose()?
        .map(|catalog| catalog.with_deezer(deezer.clone()));
    let library_connected = match source {
        MusicSource::Spotify | MusicSource::SpotifyVolta => spotify_catalog.is_some(),
        MusicSource::Volta => volta_catalog.is_some(),
        MusicSource::Youtube => tools
            .as_ref()
            .and_then(|tools| tools.music.as_ref())
            .is_some_and(|music| music.connected()),
    };
    let catalog = match source {
        MusicSource::Spotify | MusicSource::SpotifyVolta => spotify_catalog.clone(),
        MusicSource::Volta => volta_catalog.clone(),
        MusicSource::Youtube => youtube_catalog.clone(),
    }
    .unwrap_or(Catalog::offline()?)
    .with_deezer(deezer);
    let mut playback =
        Playback::spawn_monochrome(catalog.clone(), config.volume, visualizer.clone())?;
    // Wrap in priority order. Each provider receives the original queue ID and
    // resolves independently; Volta's internal YouTube retry is bypassed here.
    if let (Some(client), Some(target)) = (volta, volta_catalog) {
        let next = Playback::spawn_volta_only(client, config.volume, visualizer.clone())?
            .with_audio_catalog(target, catalog.clone());
        playback = playback.with_fallback(next, "Volta");
    }
    if let Some(target) = spotify_catalog
        && let Ok(Some(tokens)) = TokenManager::load_optional_streaming()
    {
        let next = Playback::spawn_with_visualizer(
            tokens,
            crate::auth::STREAMING_CLIENT_ID.into(),
            config.volume,
            visualizer.clone(),
        )
        .with_audio_catalog(target, catalog.clone());
        playback = playback.with_fallback(next, "Spotify");
    }
    if let (Some(tools), Some(target)) = (tools.clone(), youtube_catalog) {
        let next = Playback::spawn_youtube(tools, config.volume, visualizer)
            .with_audio_catalog(target, catalog.clone());
        playback = playback.with_fallback(next, "YouTube");
    }
    Ok(Services {
        playback: playback.with_recovery(catalog.clone()),
        catalog,
        music_catalog: source == MusicSource::Youtube
            && tools.is_some_and(|tools| tools.music.is_some()),
        library_connected,
    })
}
