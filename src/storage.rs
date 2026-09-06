use crate::{
    model::{Repeat, valid_id},
    queue::Queue,
};
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

pub fn validate_ids(ids: &[String]) -> Result<()> {
    if ids.len() > crate::queue::MAX_TRACKS || ids.iter().any(|id| !valid_id(id)) {
        bail!(
            "Invalid queue track IDs or queue exceeds 100,000 tracks; preserve queue.json and move it aside to reset"
        );
    }
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
}
