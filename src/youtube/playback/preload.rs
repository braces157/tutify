//! Resolve just the next queue entry; keep a small, expiring in-memory stream cache.
use super::*;
use std::collections::VecDeque;

pub(super) struct Resolution(JoinHandle<Result<Stream>>);

impl Drop for Resolution {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) enum Prepared {
    Ready(Box<Stream>),
    Loading(Resolution),
}

impl Prepared {
    pub(super) fn ready(&self) -> Option<Stream> {
        match self {
            Self::Ready(stream) if stream.fresh() => Some((**stream).clone()),
            _ => None,
        }
    }

    pub(super) async fn resolve(self, tools: &Tools, id: &str) -> Result<Stream> {
        let stream = match self {
            Self::Ready(stream) => Some(*stream),
            Self::Loading(mut job) => (&mut job.0).await.ok().and_then(Result::ok),
        };
        match stream.filter(Stream::fresh) {
            Some(stream) => Ok(stream),
            None => tools.resolve(id).await,
        }
    }
}

#[derive(Default)]
pub(super) struct Preparer {
    pending: Option<(String, Resolution)>,
    streams: VecDeque<Stream>,
}

impl Preparer {
    pub(super) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub(super) fn remember(&mut self, stream: Stream) {
        self.streams
            .retain(|cached| cached.track.id != stream.track.id && cached.fresh());
        if stream.fresh() {
            self.streams.push_back(stream);
        }
        while self.streams.len() > 4 {
            self.streams.pop_front();
        }
    }

    pub(super) fn request(&mut self, tools: &Tools, id: String) {
        if super::super::video_key(&id).is_none() {
            return;
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|(pending, _)| pending == &id)
        {
            return;
        }
        self.pending = None;
        self.streams.retain(Stream::fresh);
        if self.streams.iter().any(|stream| stream.track.id == id) {
            return;
        }
        let tools = tools.clone();
        let target = id.clone();
        self.pending = Some((
            id,
            Resolution(tokio::spawn(async move { tools.resolve(&target).await })),
        ));
    }

    pub(super) fn take(&mut self, id: &str) -> Option<Prepared> {
        self.streams.retain(Stream::fresh);
        if let Some(stream) = self.streams.iter().find(|stream| stream.track.id == id) {
            return Some(Prepared::Ready(Box::new(stream.clone())));
        }
        if let Some((target, job)) = self.pending.take()
            && target == id
        {
            return Some(Prepared::Loading(job));
        }
        None
    }

    pub(super) async fn collect(&mut self) {
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, job)| job.0.is_finished())
            && let Some((_, mut job)) = self.pending.take()
            && let Ok(Ok(stream)) = (&mut job.0).await
        {
            self.remember(stream);
        }
    }

    pub(super) fn forget(&mut self, id: &str) {
        self.streams.retain(|stream| stream.track.id != id);
    }

    pub(super) fn cancel(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_streams_are_never_reused_and_cache_is_bounded() {
        let mut cache = Preparer::default();
        for index in 0..10 {
            cache.remember(Stream {
                track: crate::model::Track {
                    id: format!("youtube:v{index:010}"),
                    ..Default::default()
                },
                url: "https://rr1.googlevideo.com/audio".into(),
                headers: String::new(),
                obtained_at: std::time::Instant::now(),
            });
        }
        assert_eq!(cache.streams.len(), 4);
        assert!(cache.take("youtube:v0000000000").is_none());
        assert!(cache.take("youtube:v0000000009").is_some());
        cache.streams.back_mut().unwrap().url.push_str("?expire=1");
        assert!(cache.take("youtube:v0000000009").is_none());
        cache.streams.back_mut().unwrap().obtained_at -= Duration::from_secs(301);
        assert!(cache.take("youtube:v0000000008").is_none());
        cache.forget("youtube:v0000000007");
        assert!(cache.take("youtube:v0000000007").is_none());
    }

    #[tokio::test]
    async fn selecting_an_inflight_preload_reuses_its_single_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let tools = super::super::super::tests::fixture(
            dir.path(),
            "Start-Sleep -Milliseconds 200\n[Console]::WriteLine('{\"id\":\"dQw4w9WgXcQ\",\"title\":\"Song\",\"duration\":180,\"url\":\"https://rr1.googlevideo.com/audio\"}')",
        );
        let mut cache = Preparer::default();
        cache.request(&tools, "youtube:dQw4w9WgXcQ".into());
        cache.request(&tools, "youtube:dQw4w9WgXcQ".into());
        let prepared = cache.take("youtube:dQw4w9WgXcQ").unwrap();
        assert!(matches!(prepared, Prepared::Loading(_)));
        assert!(cache.pending.is_none());
        let stream = prepared
            .resolve(&tools, "youtube:dQw4w9WgXcQ")
            .await
            .unwrap();
        cache.remember(stream);
        assert!(matches!(
            cache.take("youtube:dQw4w9WgXcQ"),
            Some(Prepared::Ready(_))
        ));
    }

    #[tokio::test]
    async fn cancelling_preparation_kills_the_owned_resolver() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("pid.txt");
        let tools = super::super::super::tests::fixture(
            dir.path(),
            &format!(
                "[IO.File]::WriteAllText('{}',[string]$PID)\nStart-Sleep -Seconds 60",
                marker.display().to_string().replace('\'', "''")
            ),
        );
        let mut cache = Preparer::default();
        cache.request(&tools, "youtube:dQw4w9WgXcQ".into());
        tokio::time::timeout(Duration::from_secs(10), async {
            while !marker.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let pid = std::fs::read_to_string(&marker)
            .unwrap()
            .parse::<u32>()
            .unwrap();
        cache.cancel();
        tokio::time::sleep(Duration::from_millis(50)).await;
        use windows::Win32::{
            Foundation::{CloseHandle, WAIT_OBJECT_0},
            System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        };
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
                assert_eq!(WaitForSingleObject(handle, 5000), WAIT_OBJECT_0);
                CloseHandle(handle).unwrap();
            }
        }
        assert!(!cache.is_pending());
    }
}
