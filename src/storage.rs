use crate::{model::Repeat, queue::Queue};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

mod backup;
mod recovery;
pub(crate) use backup::confirmation_token;
pub(crate) use recovery::StateFile;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(skip)]
    pub youtube_music: bool,
    #[serde(skip)]
    pub youtube_connected: bool,
    #[serde(skip)]
    pub source: crate::model::MusicSource,
    pub version: u32,
    pub client_id: String,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub theme: String,
    pub background_image: Option<String>,
    pub background_image_vertical: Option<String>,
    pub background_dim: u8,
    #[serde(skip)]
    pub native_glass: bool,
    pub discord_rpc: bool,
    pub discord_client_id: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            youtube_music: false,
            youtube_connected: false,
            source: crate::model::MusicSource::Spotify,
            version: 1,
            client_id: String::new(),
            volume: 50,
            shuffle: false,
            repeat: Repeat::Off,
            theme: "spotify".into(),
            background_image: None,
            background_image_vertical: None,
            background_dim: 38,
            native_glass: false,
            discord_rpc: true,
            discord_client_id: None,
        }
    }
}

#[derive(Clone)]
pub struct Storage {
    pub root: PathBuf,
}

impl Storage {
    /// Separate provider data, with inherited appearance on first launch.
    pub fn youtube(&self) -> Result<Self> {
        let store = Self {
            root: self.root.join("youtube"),
        };
        fs::create_dir_all(&store.root)?;
        if !store.root.join("config.json").exists() {
            let mut config = self.config()?;
            config.client_id.clear();
            store.save_config(&config)?;
        }
        Ok(store)
    }
    pub fn lock(&self) -> Result<fs::File> {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join("instance.lock"))?;
        fs2::FileExt::try_lock_exclusive(&file).context("Tuitify is already running; close it before opening another player or changing saved state")?;
        backup::recover_pending(self)?;
        Ok(file)
    }
    pub fn local() -> Result<Self> {
        let store = Self::local_read_only()?;
        fs::create_dir_all(&store.root)?;
        Ok(store)
    }
    /// Resolve the normal data root without creating it or recovering a journal.
    pub(crate) fn local_read_only() -> Result<Self> {
        let root = PathBuf::from(
            std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set; run on Windows")?,
        )
        .join("Tuitify");
        Ok(Self { root })
    }
    pub fn config(&self) -> Result<Config> {
        let c: Config = recovery::load(self, StateFile::Config)?;
        recovery::validate_config(&c)
            .map_err(|error| recovery::invariant(StateFile::Config, error.to_string()))?;
        Ok(c)
    }
    pub fn queue(&self) -> Result<Queue> {
        let q: Queue = recovery::load(self, StateFile::Queue)?;
        q.validate()
            .map_err(|error| recovery::invariant(StateFile::Queue, error.to_string()))?;
        Ok(q)
    }
    pub fn cache(&self) -> Result<crate::cache::MetadataCache> {
        let mut cache: crate::cache::MetadataCache = recovery::load(self, StateFile::Cache)?;
        cache
            .validate()
            .map_err(|error| recovery::invariant(StateFile::Cache, error.to_string()))?;
        Ok(cache)
    }
    #[cfg(test)]
    pub fn save(&self, config: &Config, queue: &Queue) -> Result<()> {
        self.save_config(config)?;
        self.save_queue(queue)
    }
    pub fn save_queue(&self, queue: &Queue) -> Result<()> {
        queue.validate()?;
        recovery::protect_future_version(self, StateFile::Queue)?;
        atomic_json(&self.root.join("queue.json"), queue)
    }
    pub fn save_cache(&self, cache: &crate::cache::MetadataCache) -> Result<()> {
        recovery::protect_future_version(self, StateFile::Cache)?;
        atomic_json(&self.root.join("cache.json"), cache)
    }
    pub fn clear_cache(&self) -> Result<()> {
        match fs::remove_file(self.root.join("cache.json")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save_config(&self, config: &Config) -> Result<()> {
        recovery::protect_future_version(self, StateFile::Config)?;
        atomic_json(&self.root.join("config.json"), config)
    }
    pub(crate) fn save_launch_plan<T: Serialize>(&self, plan: &T) -> Result<()> {
        atomic_json(&self.root.join("launch-plan.json"), plan)
    }
    pub fn clear_queue(&self) -> Result<()> {
        match fs::remove_file(self.root.join("queue.json")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn stats(&self) -> Result<crate::stats::SongStats> {
        let stats: crate::stats::SongStats = recovery::load(self, StateFile::Stats)?;
        recovery::validate_stats(&stats)
            .map_err(|error| recovery::invariant(StateFile::Stats, error.to_string()))?;
        Ok(stats)
    }
    pub fn save_stats(&self, stats: &crate::stats::SongStats) -> Result<()> {
        stats.validate()?;
        recovery::protect_future_version(self, StateFile::Stats)?;
        atomic_json(&self.root.join("stats.json"), stats)
    }
    pub fn mix_recipes(&self) -> Result<crate::mix::MixRecipes> {
        let recipes: crate::mix::MixRecipes = recovery::load(self, StateFile::Recipes)?;
        recipes
            .validate()
            .map_err(|error| recovery::invariant(StateFile::Recipes, error.to_string()))?;
        Ok(recipes)
    }
    pub fn save_mix_recipes(&self, recipes: &crate::mix::MixRecipes) -> Result<()> {
        recipes.validate()?;
        recovery::protect_future_version(self, StateFile::Recipes)?;
        atomic_json(&self.root.join("mix-recipes.json"), recipes)
    }
    pub fn clear_stats(&self) -> Result<()> {
        match fs::remove_file(self.root.join("stats.json")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut tmp =
        tempfile::NamedTempFile::new_in(path.parent().context("Missing parent directory")?)?;
    // JSON emits many small tokens. Buffer them before touching the filesystem,
    // and flush explicitly so a write failure cannot publish a partial snapshot.
    {
        let mut output = BufWriter::new(&mut tmp);
        serde_json::to_writer_pretty(&mut output, value)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    tmp.as_file().sync_all()?;
    // tempfile uses MoveFileExW with REPLACE_EXISTING on Windows, on the same volume.
    tmp.persist(path)
        .map_err(|e| e.error)
        .with_context(|| format!("Cannot save {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serialization_failure_preserves_committed_snapshot() {
        struct Failing;
        impl Serialize for Failing {
            fn serialize<S: serde::Serializer>(
                &self,
                _: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("failed snapshot"))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        atomic_json(&path, &vec!["committed"]).unwrap();
        let before = fs::read(&path).unwrap();
        assert!(atomic_json(&path, &Failing).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    #[ignore = "Release microbenchmark; run with --release --ignored --nocapture"]
    fn benchmark_state_save() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage {
            root: dir.path().to_owned(),
        };
        for count in [5_000, 50_000] {
            let mut queue = Queue::default();
            queue.replace((0..count).map(|i| format!("{i:022}")).collect(), 0, false);
            let start = std::time::Instant::now();
            for _ in 0..3 {
                store.save_queue(&queue).unwrap();
            }
            println!(
                "queue_rows={count} mean_save_ms={:.3}",
                start.elapsed().as_secs_f64() * 1000.0 / 3.0
            );
            assert_eq!(store.queue().unwrap().ids, queue.ids);
        }
    }
    #[test]
    fn cache_roundtrips_and_clear_preserves_settings_and_queue() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage {
            root: dir.path().to_owned(),
        };
        let mut cache = crate::cache::MetadataCache::default();
        cache.insert(
            "0".repeat(22),
            crate::model::Track::unknown(&"0".repeat(22)),
        );
        store.save_cache(&cache).unwrap();
        store.save(&Config::default(), &Queue::default()).unwrap();
        assert!(store.cache().unwrap().contains_key(&"0".repeat(22)));
        store.clear_cache().unwrap();
        store.clear_cache().unwrap();
        assert!(!store.root.join("cache.json").exists());
        assert!(store.root.join("config.json").exists());
        assert!(store.root.join("queue.json").exists());
    }
    #[test]
    fn roundtrip_and_corruption_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage {
            root: dir.path().to_owned(),
        };
        let mut q = Queue::default();
        q.replace(vec!["0".repeat(22)], 0, false);
        q.position_ms = 1234;
        store.save(&Config::default(), &q).unwrap();
        store
            .save(
                &Config {
                    volume: 71,
                    ..Config::default()
                },
                &q,
            )
            .unwrap();
        assert_eq!(store.config().unwrap().volume, 71);
        assert_eq!(store.queue().unwrap().position_ms, 1234);
        assert!(
            !fs::read_to_string(dir.path().join("queue.json"))
                .unwrap()
                .contains("name")
        );
        fs::write(dir.path().join("queue.json"), b"broken").unwrap();
        assert!(store.queue().is_err());
        assert_eq!(fs::read(dir.path().join("queue.json")).unwrap(), b"broken");
        store.clear_queue().unwrap();
        assert!(store.queue().unwrap().ids.is_empty());
    }
    #[test]
    fn config_discord_defaults_and_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage {
            root: dir.path().to_owned(),
        };
        // Verify default config has discord_rpc enabled
        let default_config = Config::default();
        assert!(default_config.discord_rpc);
        assert_eq!(default_config.discord_client_id, None);

        // Older config without discord fields should deserialize with defaults
        let older_json = r#"{"version":1,"client_id":"test","volume":60}"#;
        fs::write(dir.path().join("config.json"), older_json).unwrap();
        let loaded = store.config().unwrap();
        assert!(loaded.discord_rpc);
        assert_eq!(loaded.discord_client_id, None);
        assert_eq!(loaded.volume, 60);

        // Custom config saves and loads properly
        let custom = Config {
            discord_rpc: false,
            discord_client_id: Some("123456789".into()),
            ..Default::default()
        };
        store.save_config(&custom).unwrap();
        let loaded_custom = store.config().unwrap();
        assert!(!loaded_custom.discord_rpc);
        assert_eq!(
            loaded_custom.discord_client_id.as_deref(),
            Some("123456789")
        );
    }
    #[test]
    fn instance_lock_releases_and_uncommitted_temp_does_not_replace_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage {
            root: dir.path().to_owned(),
        };
        let lock = store.lock().unwrap();
        assert!(store.lock().is_err());
        drop(lock);
        assert!(store.lock().is_ok());
        store.save(&Config::default(), &Queue::default()).unwrap();
        fs::write(dir.path().join("abandoned.tmp"), b"incomplete write").unwrap();
        assert!(store.queue().unwrap().ids.is_empty());
        fs::write(dir.path().join("queue.json"), br#"{"version":999}"#).unwrap();
        assert!(store.queue().is_err());
    }
    #[test]
    fn stats_roundtrips_and_clear_preserves_settings_and_queue() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage {
            root: dir.path().to_owned(),
        };
        let mut stats = crate::stats::SongStats::default();
        let id = "0".repeat(22);
        stats.add_listened_ms(&id, 12345, "Track Name", "Artist Name");
        stats.add_play(&id, "Track Name", "Artist Name");
        store.save_stats(&stats).unwrap();
        store.save(&Config::default(), &Queue::default()).unwrap();

        let loaded = store.stats().unwrap();
        assert_eq!(loaded.len(), 1);
        let s = loaded.tracks.get(&id).unwrap();
        assert_eq!(s.play_count, 1);
        assert_eq!(s.listened_ms, 12345);

        store.clear_stats().unwrap();
        store.clear_stats().unwrap();
        assert!(!store.root.join("stats.json").exists());
        assert!(store.root.join("config.json").exists());
        assert!(store.root.join("queue.json").exists());
        assert_eq!(store.stats().unwrap().len(), 0);
    }
    #[test]
    fn stats_corruption_and_bounds() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage {
            root: dir.path().to_owned(),
        };
        fs::write(dir.path().join("stats.json"), b"corrupted json").unwrap();
        assert!(store.stats().is_err());
        assert_eq!(
            fs::read(dir.path().join("stats.json")).unwrap(),
            b"corrupted json"
        );

        fs::write(
            dir.path().join("stats.json"),
            br#"{"version":2,"tracks":{}}"#,
        )
        .unwrap();
        assert!(store.stats().is_err());
    }
    #[test]
    fn mix_recipes_are_separate_from_queue_and_settings() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage {
            root: dir.path().to_owned(),
        };
        let mut recipes = crate::mix::MixRecipes::default();
        recipes.save(crate::mix::MixRecipe {
            name: "Commute".into(),
            source: crate::mix::MixSource::Queue,
            settings: crate::mix::MixSettings::default(),
        });
        store.save_mix_recipes(&recipes).unwrap();
        assert_eq!(store.mix_recipes().unwrap().recipes.len(), 1);
        assert!(!store.root.join("queue.json").exists());
        assert!(!store.root.join("config.json").exists());
    }

    #[test]
    fn mix_recipe_write_failures_and_invalid_persisted_values_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage {
            root: dir.path().to_owned(),
        };
        fs::create_dir(dir.path().join("mix-recipes.json")).unwrap();
        assert!(
            store
                .save_mix_recipes(&crate::mix::MixRecipes::default())
                .is_err()
        );

        fs::remove_dir(dir.path().join("mix-recipes.json")).unwrap();
        fs::write(
            dir.path().join("mix-recipes.json"),
            br#"{"version":1,"recipes":[{"name":"Bad","source":{"Playlist":{"id":"short","name":"List"}},"settings":{"target_minutes":0,"recommendation_percent":101,"artist_gap":21}}]}"#,
        )
        .unwrap();
        assert!(store.mix_recipes().is_err());
    }

    #[test]
    fn config_responsive_background_fields_roundtrip_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = Storage {
            root: dir.path().to_owned(),
        };
        // Older config without background_image_vertical should deserialize with None
        let older_json = r#"{"version":1,"client_id":"test","theme":"glass","background_image":"landscape.jpg"}"#;
        fs::write(dir.path().join("config.json"), older_json).unwrap();
        let loaded = store.config().unwrap();
        assert_eq!(loaded.background_image.as_deref(), Some("landscape.jpg"));
        assert_eq!(loaded.background_image_vertical, None);

        // Roundtrip with both fields
        let mut custom = loaded;
        custom.background_image_vertical = Some("portrait.jpg".into());
        store.save_config(&custom).unwrap();
        let reloaded = store.config().unwrap();
        assert_eq!(reloaded.background_image.as_deref(), Some("landscape.jpg"));
        assert_eq!(
            reloaded.background_image_vertical.as_deref(),
            Some("portrait.jpg")
        );
    }
}
