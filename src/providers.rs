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
    match source {
        MusicSource::Spotify => Ok(Services {
            catalog: Catalog::new(TokenManager::load(config)?)?,
            playback: Playback::spawn_with_visualizer(
                TokenManager::load_streaming()?,
                config.client_id.clone(),
                config.volume,
                visualizer,
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
                playback: Playback::spawn_youtube(tools, config.volume, visualizer),
                music_catalog,
                library_connected,
            })
        }
    }
}
