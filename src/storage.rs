use crate::{model::Repeat, queue::Queue};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub client_id: String,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub theme: String,
    pub discord_rpc: bool,
    pub discord_client_id: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            client_id: String::new(),
            volume: 50,
            shuffle: false,
            repeat: Repeat::Off,
            theme: "spotify".into(),
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
    pub fn lock(&self) -> Result<fs::File> {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join("instance.lock"))?;
        fs2::FileExt::try_lock_exclusive(&file).context("Tuitify is already running; close it before opening another player, logging in, or logging out")?;
        Ok(file)
    }
    pub fn local() -> Result<Self> {
        let root = PathBuf::from(
            std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set; run on Windows")?,
        )
        .join("Tuitify");
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }
    pub fn config(&self) -> Result<Config> {
        let c: Config = read_or_default(&self.root.join("config.json"))?;
        if c.version != 1 {
            bail!(
                "Unsupported config version {}; preserve config.json and update Tuitify",
                c.version
            );
        }
        Ok(Config {
            volume: c.volume.min(100),
            ..c
        })
    }
    pub fn queue(&self) -> Result<Queue> {
        let q: Queue = read_or_default(&self.root.join("queue.json"))?;
        q.validate()?;
        Ok(q)
    }
    pub fn cache(&self) -> Result<crate::cache::MetadataCache> {
        let mut cache: crate::cache::MetadataCache =
            read_or_default(&self.root.join("cache.json"))?;
        cache.validate()?;
        Ok(cache)
    }
    #[cfg(test)]
    pub fn save(&self, config: &Config, queue: &Queue) -> Result<()> {
        self.save_config(config)?;
        self.save_queue(queue)
    }
    pub fn save_queue(&self, queue: &Queue) -> Result<()> {
        queue.validate()?;
        atomic_json(&self.root.join("queue.json"), queue)
    }
    pub fn save_cache(&self, cache: &crate::cache::MetadataCache) -> Result<()> {
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
        atomic_json(&self.root.join("config.json"), config)
    }
    pub fn clear_queue(&self) -> Result<()> {
        match fs::remove_file(self.root.join("queue.json")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn stats(&self) -> Result<crate::stats::SongStats> {
        let mut stats: crate::stats::SongStats = read_or_default(&self.root.join("stats.json"))?;
        stats.validate()?;
        Ok(stats)
    }
    pub fn save_stats(&self, stats: &crate::stats::SongStats) -> Result<()> {
        let mut stats = stats.clone();
        stats.validate()?;
        atomic_json(&self.root.join("stats.json"), &stats)
    }
    pub fn clear_stats(&self) -> Result<()> {
        match fs::remove_file(self.root.join("stats.json")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

fn read_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "Cannot read {}; move this file aside to reset it (it has been preserved)",
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e.into()),
    }
}

fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut tmp =
        tempfile::NamedTempFile::new_in(path.parent().context("Missing parent directory")?)?;
    serde_json::to_writer_pretty(&mut tmp, value)?;
    tmp.write_all(b"\n")?;
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
}
