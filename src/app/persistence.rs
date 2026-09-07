use super::*;

/// Each independent file has a coalescing writer. Failed snapshots are retried
/// by the next checkpoint; the final failure is returned after terminal cleanup.
pub(super) fn writer<T, F>(
    mut rx: watch::Receiver<Option<T>>,
    tx: mpsc::UnboundedSender<Background>,
    save: F,
) -> tokio::task::JoinHandle<Result<()>>
where
    T: Clone + Send + Sync + 'static,
    F: Fn(T) -> Result<()> + Send + Sync + 'static,
{
    let save = Arc::new(save);
    tokio::spawn(async move {
        let mut last_error = None;
        while rx.changed().await.is_ok() {
            let value = rx.borrow_and_update().clone();
            let Some(value) = value else {
                continue;
            };
            let save = save.clone();
            match tokio::task::spawn_blocking(move || save(value)).await {
                Ok(Ok(())) => last_error = None,
                result => {
                    let error = match result {
                        Ok(Err(e)) => format!("Could not save state: {e:#}"),
                        Err(e) => format!("State writer failed: {e}"),
                        _ => unreachable!(),
                    };
                    let _ = tx.send(Background::SaveError(error.clone()));
                    last_error = Some(error);
                }
            }
        }
        if let Some(e) = last_error {
            anyhow::bail!(e);
        }
        Ok(())
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct QueueStamp(u64, Option<usize>, usize, u32);
pub(super) fn queue_stamp(queue: &Queue) -> QueueStamp {
    QueueStamp(
        queue.revision,
        queue.cursor,
        queue.selected,
        queue.position_ms,
    )
}
pub(super) struct Checkpoints {
    pub(super) config: Config,
    pub(super) queue: QueueStamp,
    pub(super) cache: u64,
    pub(super) stats: u64,
    pub(super) retry: bool,
    pub(super) config_tx: watch::Sender<Option<Config>>,
    pub(super) queue_tx: watch::Sender<Option<Queue>>,
    pub(super) cache_tx: watch::Sender<Option<crate::cache::MetadataCache>>,
    pub(super) stats_tx: watch::Sender<Option<crate::stats::SongStats>>,
}
impl Checkpoints {
    pub(super) fn send(&mut self, app: &App) {
        if self.retry || self.config != app.config {
            self.config_tx.send_replace(Some(app.config.clone()));
            self.config = app.config.clone();
        }
        let stamp = queue_stamp(&app.queue);
        if self.retry || self.queue != stamp {
            self.queue_tx.send_replace(Some(app.queue.clone()));
            self.queue = stamp;
        }
        if self.retry || self.cache != app.cache.revision {
            self.cache_tx.send_replace(Some(app.cache.clone()));
            self.cache = app.cache.revision;
        }
        if self.retry || self.stats != app.stats.revision {
            self.stats_tx.send_replace(Some(app.stats.clone()));
            self.stats = app.stats.revision;
        }
        self.retry = false;
    }
}
