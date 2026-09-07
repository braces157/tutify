use super::*;

#[test]
fn checkpoint_sends_only_changed_components() {
    let mut app = App::new(Config::default(), Queue::default());
    let (config_tx, config_rx) = watch::channel(None);
    let (queue_tx, mut queue_rx) = watch::channel(None);
    let (cache_tx, cache_rx) = watch::channel(None);
    let (stats_tx, stats_rx) = watch::channel(None);
    let mut checkpoints = Checkpoints {
        config: app.config.clone(),
        queue: queue_stamp(&app.queue),
        cache: app.cache.revision,
        stats: app.stats.revision,
        retry: false,
        config_tx,
        queue_tx,
        cache_tx,
        stats_tx,
    };
    app.catalog.query = "typing".into();
    app.catalog.selected = 5;
    checkpoints.send(&app);
    assert!(!config_rx.has_changed().unwrap());
    assert!(!queue_rx.has_changed().unwrap());
    assert!(!cache_rx.has_changed().unwrap());
    assert!(!stats_rx.has_changed().unwrap());
    app.queue.enqueue(test_track(1).id);
    checkpoints.send(&app);
    assert!(queue_rx.has_changed().unwrap());
    queue_rx.borrow_and_update();
    app.config.volume = 31;
    checkpoints.send(&app);
    assert!(config_rx.has_changed().unwrap());
    assert!(!queue_rx.has_changed().unwrap());
    assert!(!cache_rx.has_changed().unwrap());
    assert!(!stats_rx.has_changed().unwrap());
    app.cache.insert(test_track(1).id, test_track(1));
    checkpoints.send(&app);
    assert!(cache_rx.has_changed().unwrap());
    assert!(!stats_rx.has_changed().unwrap());
    app.stats
        .add_listened_ms(&"0".repeat(22), 1000, "Song", "Artist");
    checkpoints.send(&app);
    assert!(stats_rx.has_changed().unwrap());
}
#[tokio::test]
async fn writer_failure_is_reported_and_later_snapshot_can_recover() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let (tx, rx) = watch::channel(None);
    let (bg, mut errors) = mpsc::unbounded_channel();
    let (done, mut completed) = mpsc::unbounded_channel();
    let writer = writer(rx, bg, move |value: u8| {
        if count.fetch_add(1, Ordering::SeqCst) == 0 {
            anyhow::bail!("disk unavailable");
        }
        done.send(value).unwrap();
        Ok(())
    });
    tx.send_replace(Some(1));
    assert!(matches!(
        errors.recv().await.unwrap(),
        Background::SaveError(_)
    ));
    tx.send_replace(Some(2));
    assert_eq!(completed.recv().await, Some(2));
    drop(tx);
    writer.await.unwrap().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn writer_flushes_pending_final_value_and_returns_final_error() {
    let (tx, rx) = watch::channel(None);
    let (bg, _) = mpsc::unbounded_channel();
    let saved = Arc::new(std::sync::Mutex::new(Vec::new()));
    let output = saved.clone();
    let task = writer(rx, bg, move |value: u8| {
        output.lock().unwrap().push(value);
        Ok(())
    });
    tx.send_replace(Some(9));
    drop(tx);
    task.await.unwrap().unwrap();
    assert_eq!(*saved.lock().unwrap(), vec![9]);
    let (tx, rx) = watch::channel(None);
    let (bg, _) = mpsc::unbounded_channel();
    let task = writer(rx, bg, |_: u8| anyhow::bail!("final disk failure"));
    tx.send_replace(Some(1));
    drop(tx);
    assert!(
        task.await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("final disk failure")
    );
}
#[tokio::test]
async fn stats_save_error_updates_status_and_triggers_retry() {
    let mut app = App::new(Config::default(), Queue::default());
    let (mut tasks, _) = tasks();
    let retry = background(
        &mut app,
        &mut tasks,
        Background::SaveError("Could not save state: stats write error".into()),
    );
    assert!(retry);
    assert!(app.status.contains("stats write error"));
}
