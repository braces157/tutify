//! A metadata-only Windows media session. Librespot remains the audio owner.
use crate::{app::State, model::Track};
use std::{sync::mpsc, thread};
use tokio::sync::mpsc::UnboundedSender;
use windows::{
    Foundation::{TimeSpan, TypedEventHandler},
    Media::{
        MediaPlaybackStatus, MediaPlaybackType, Playback::MediaPlayer,
        SystemMediaTransportControls, SystemMediaTransportControlsButton,
        SystemMediaTransportControlsButtonPressedEventArgs,
        SystemMediaTransportControlsTimelineProperties,
    },
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
}

pub enum Event {
    Action(Action),
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub track: Option<Track>,
    pub state: State,
    pub position_ms: u32,
}

pub struct MediaControls {
    tx: Option<mpsc::SyncSender<Snapshot>>,
    thread: Option<thread::JoinHandle<()>>,
    last: Option<Snapshot>,
}

impl MediaControls {
    pub fn spawn(events: UnboundedSender<Event>) -> std::io::Result<Self> {
        // Bound updates if Windows is slow; the UI retries the latest snapshot.
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("windows-media".into())
            .spawn(move || {
                if run(rx, events.clone()).is_err() {
                    let _ = events.send(Event::Unavailable);
                }
            })?;
        Ok(Self {
            tx: Some(tx),
            thread: Some(thread),
            last: None,
        })
    }

    pub fn update(&mut self, mut snapshot: Snapshot) {
        // Publish at most once per second during steady playback.
        snapshot.position_ms = snapshot.position_ms / 1000 * 1000;
        if self.last.as_ref() != Some(&snapshot)
            && self
                .tx
                .as_ref()
                .is_some_and(|tx| tx.try_send(snapshot.clone()).is_ok())
        {
            self.last = Some(snapshot);
        }
    }
}

impl Drop for MediaControls {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            RoUninitialize();
        }
    }
}

struct Session {
    player: MediaPlayer,
    controls: SystemMediaTransportControls,
    button_token: Option<i64>,
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Some(token) = self.button_token {
            let _ = self.controls.RemoveButtonPressed(token);
        }
        let _ = self.controls.SetIsEnabled(false);
        let _ = self.player.Close();
    }
}

fn run(rx: mpsc::Receiver<Snapshot>, events: UnboundedSender<Event>) -> windows::core::Result<()> {
    // All WinRT objects are created and released on this dedicated MTA thread.
    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED)?;
    }
    let _apartment = Apartment;
    let player = MediaPlayer::new()?;
    let controls = player.SystemMediaTransportControls()?;
    let mut session = Session {
        player,
        controls,
        button_token: None,
    };
    session.player.CommandManager()?.SetIsEnabled(false)?;
    let controls = &session.controls;
    controls.SetIsEnabled(false)?;
    controls.SetIsPlayEnabled(true)?;
    controls.SetIsPauseEnabled(true)?;
    controls.SetIsNextEnabled(true)?;
    controls.SetIsPreviousEnabled(true)?;
    session.button_token = Some(controls.ButtonPressed(&TypedEventHandler::<
        SystemMediaTransportControls,
        SystemMediaTransportControlsButtonPressedEventArgs,
    >::new(move |_, args| {
        if let Some(args) = args.as_ref() {
            let action = match args.Button()? {
                SystemMediaTransportControlsButton::Play => Some(Action::Play),
                SystemMediaTransportControlsButton::Pause => Some(Action::Pause),
                SystemMediaTransportControlsButton::Next => Some(Action::Next),
                SystemMediaTransportControlsButton::Previous => Some(Action::Previous),
                _ => None,
            };
            if let Some(action) = action {
                let _ = events.send(Event::Action(action));
            }
        }
        Ok(())
    }))?);
    let mut last_track = None;
    while let Ok(snapshot) = rx.recv() {
        let updater = controls.DisplayUpdater()?;
        if snapshot.track != last_track {
            updater.ClearAll()?;
            if let Some(track) = &snapshot.track {
                updater.SetType(MediaPlaybackType::Music)?;
                let music = updater.MusicProperties()?;
                music.SetTitle(&track.name.as_str().into())?;
                music.SetArtist(&track.artists.as_str().into())?;
                if let Some(album) = &track.album {
                    let _ = music.SetAlbumTitle(&album.as_str().into());
                }
            }
            updater.Update()?;
            last_track = snapshot.track.clone();
        }
        controls.SetPlaybackStatus(if snapshot.track.is_none() {
            MediaPlaybackStatus::Stopped
        } else {
            match snapshot.state {
                State::Playing => MediaPlaybackStatus::Playing,
                State::Loading => MediaPlaybackStatus::Changing,
                State::Paused | State::Failed => MediaPlaybackStatus::Paused,
            }
        })?;
        let duration = snapshot.track.as_ref().map_or(0, |t| t.duration_ms);
        let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
        timeline.SetStartTime(timespan(0))?;
        timeline.SetMinSeekTime(timespan(0))?;
        timeline.SetEndTime(timespan(duration))?;
        timeline.SetMaxSeekTime(timespan(duration))?;
        timeline.SetPosition(timespan(snapshot.position_ms.min(duration)))?;
        controls.UpdateTimelineProperties(&timeline)?;
        controls.SetIsEnabled(snapshot.track.is_some())?;
    }
    Ok(())
}

fn timespan(ms: u32) -> TimeSpan {
    TimeSpan {
        Duration: i64::from(ms) * 10_000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;

    #[test]
    fn snapshots_coalesce_and_retry_when_worker_is_busy() {
        let (tx, rx) = mpsc::sync_channel(1);
        let mut controls = MediaControls {
            tx: Some(tx),
            thread: None,
            last: None,
        };
        let mut snapshot = Snapshot {
            track: None,
            state: State::Paused,
            position_ms: 1234,
        };
        controls.update(snapshot.clone());
        snapshot.position_ms = 1999;
        controls.update(snapshot.clone());
        snapshot.state = State::Playing;
        controls.update(snapshot.clone());
        assert_eq!(rx.try_recv().unwrap().state, State::Paused);
        assert!(rx.try_recv().is_err());
        controls.update(snapshot);
        let latest = rx.try_recv().unwrap();
        assert_eq!(latest.state, State::Playing);
        assert_eq!(latest.position_ms, 1000);
        assert_eq!(timespan(u32::MAX).Duration, i64::from(u32::MAX) * 10_000);
    }

    #[test]
    #[ignore = "creates a silent Windows media session; requires an interactive Windows desktop"]
    fn windows_media_session_acceptance() -> anyhow::Result<()> {
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)?;
        }
        let _apartment = Apartment;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut controls = MediaControls::spawn(tx)?;
        let title = format!("Tuitify media acceptance {}", std::process::id());
        let mut snapshot = Snapshot {
            track: Some(Track {
                id: "0000000000000000000001".into(),
                name: title.clone(),
                artists: "Tuitify test".into(),
                duration_ms: 180000,
                playable: true,
                ..Default::default()
            }),
            state: State::Playing,
            position_ms: 12000,
        };
        controls.update(snapshot.clone());
        let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let session = loop {
            let mut found = None;
            for session in manager.GetSessions()? {
                if session.TryGetMediaPropertiesAsync()?.get()?.Title()? == title {
                    found = Some(session);
                    break;
                }
            }
            if let Some(session) = found {
                break session;
            }
            anyhow::ensure!(
                !matches!(rx.try_recv(), Ok(Event::Unavailable)),
                "Windows registration failed"
            );
            anyhow::ensure!(
                Instant::now() < deadline,
                "Windows did not expose the media session"
            );
            thread::sleep(Duration::from_millis(100));
        };
        assert_eq!(
            session
                .TryGetMediaPropertiesAsync()?
                .get()?
                .Artist()?
                .to_string(),
            "Tuitify test"
        );
        assert_eq!(
            session.GetTimelineProperties()?.Position()?.Duration,
            timespan(12000).Duration
        );
        assert_eq!(session.GetPlaybackInfo()?.PlaybackStatus()?,
            windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
        for action in [Action::Pause, Action::Play, Action::Next, Action::Previous] {
            // Simulate the app acknowledging prior commands. Windows suppresses
            // Play for a session that still reports it is already playing.
            snapshot.state = if action == Action::Play {
                State::Paused
            } else {
                State::Playing
            };
            controls.update(snapshot.clone());
            let expected = if snapshot.state == State::Paused {
                windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused
            } else {
                windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing
            };
            let deadline = Instant::now() + Duration::from_secs(5);
            while session.GetPlaybackInfo()?.PlaybackStatus()? != expected {
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "Windows playback state was not updated"
                );
                thread::sleep(Duration::from_millis(50));
            }
            assert!(match action {
                Action::Pause => session.TryPauseAsync()?.get()?,
                Action::Play => session.TryPlayAsync()?.get()?,
                Action::Next => session.TrySkipNextAsync()?.get()?,
                Action::Previous => session.TrySkipPreviousAsync()?.get()?,
                Action::Toggle => unreachable!(),
            });
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Ok(Event::Action(received)) = rx.try_recv() {
                    assert_eq!(received, action);
                    break;
                }
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "Windows {action:?} command was not delivered"
                );
                thread::sleep(Duration::from_millis(50));
            }
        }
        drop(controls);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut present = false;
            for session in manager.GetSessions()? {
                present |= session.TryGetMediaPropertiesAsync()?.get()?.Title()? == title;
            }
            if !present {
                break;
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "Media session remained after cleanup"
            );
            thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }
}
