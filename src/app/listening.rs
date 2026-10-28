use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListeningTool {
    Sleep15,
    Sleep30,
    Sleep45,
    Sleep60,
    StopAfterTrack,
    CancelSleep,
    RemovePlayed,
    RemoveDuplicates,
    RemoveUnavailable,
    ConnectSpotify,
    ConnectGoogle,
    UpdatePlayback,
}

impl ListeningTool {
    pub const ALL: [Self; 12] = [
        Self::Sleep15,
        Self::Sleep30,
        Self::Sleep45,
        Self::Sleep60,
        Self::StopAfterTrack,
        Self::CancelSleep,
        Self::RemovePlayed,
        Self::RemoveDuplicates,
        Self::RemoveUnavailable,
        Self::ConnectSpotify,
        Self::ConnectGoogle,
        Self::UpdatePlayback,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sleep15 => "Sleep in 15 minutes",
            Self::Sleep30 => "Sleep in 30 minutes",
            Self::Sleep45 => "Sleep in 45 minutes",
            Self::Sleep60 => "Sleep in 60 minutes",
            Self::StopAfterTrack => "Stop after current track",
            Self::CancelSleep => "Cancel sleep timer",
            Self::RemovePlayed => "Remove played queue entries",
            Self::RemoveDuplicates => "Remove upcoming duplicates",
            Self::RemoveUnavailable => "Remove unavailable tracks",
            Self::ConnectSpotify => "Connect Spotify account",
            Self::ConnectGoogle => "Connect Google music library",
            Self::UpdatePlayback => "Update playback tools",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Sleep15 => {
                "Pause in 15 minutes, keeping your queue and position. Enter applies; Esc closes."
            }
            Self::Sleep30 => {
                "Pause in 30 minutes, keeping your queue and position. Enter applies; Esc closes."
            }
            Self::Sleep45 => {
                "Pause in 45 minutes, keeping your queue and position. Enter applies; Esc closes."
            }
            Self::Sleep60 => {
                "Pause in 60 minutes, keeping your queue and position. Enter applies; Esc closes."
            }
            Self::StopAfterTrack => {
                "Finish the loaded track, then stop even with Repeat enabled. Changing tracks cancels it."
            }
            Self::CancelSleep => "Cancel the timer without changing playback or volume.",
            Self::RemovePlayed => {
                "Remove entries before the current track in the full queue. Current track stays; u undoes."
            }
            Self::RemoveDuplicates => {
                "Keep one occurrence of each upcoming track ID, excluding the current track. u undoes."
            }
            Self::RemoveUnavailable => {
                "Remove known unavailable entries from the full queue. Current and unknown tracks stay; u undoes."
            }
            Self::ConnectSpotify => {
                "Sign in in your browser. Premium plays Spotify; free accounts use free music automatically. Your queue is saved first."
            }
            Self::ConnectGoogle => {
                "Sign in in your browser for playlists and Liked Songs. Playback needs no paid subscription. Your queue is saved first."
            }
            Self::UpdatePlayback => {
                "Update playback dependencies, then return to your saved queue. No launch options are needed."
            }
        }
    }

    pub(super) fn aliases(self) -> &'static str {
        match self {
            Self::Sleep15 | Self::Sleep30 | Self::Sleep45 | Self::Sleep60 => "timer pause bedtime",
            Self::StopAfterTrack => "sleep timer finish stop",
            Self::CancelSleep => "timer off disable",
            Self::RemovePlayed => "cleanup history queue",
            Self::RemoveDuplicates => "cleanup repeated queue",
            Self::RemoveUnavailable => "cleanup unplayable queue",
            Self::ConnectSpotify => "account login sign in premium",
            Self::ConnectGoogle => "account login sign in youtube playlists liked songs",
            Self::UpdatePlayback => "repair dependencies download yt-dlp deno",
        }
    }

    pub(super) fn unavailable(self, app: &App) -> Option<&'static str> {
        match self {
            Self::StopAfterTrack
                if !app.loaded || app.queue.current().is_none() || app.state == State::Failed =>
            {
                Some("Start a track before choosing stop after current track.")
            }
            Self::ConnectSpotify | Self::ConnectGoogle | Self::UpdatePlayback if app.demo => {
                Some("Account connections are unavailable in the offline demo.")
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SleepTimer {
    #[default]
    Off,
    Deadline(Instant),
    EndOfTrack(u64),
}

#[derive(Default)]
pub struct ListeningTools {
    pub selected: usize,
    pub sleep: SleepTimer,
}

impl SleepTimer {
    pub fn label(self, now: Instant) -> Option<String> {
        match self {
            Self::Off => None,
            Self::EndOfTrack(_) => Some("End track".into()),
            Self::Deadline(at) => {
                let left = at.saturating_duration_since(now);
                let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
                Some(format!("Sleep {}:{:02}", seconds / 60, seconds % 60))
            }
        }
    }
    pub(super) fn refresh_at(self, now: Instant) -> Option<Instant> {
        match self {
            Self::Deadline(at) => Some(at.min(now + Duration::from_secs(1))),
            _ => None,
        }
    }
}

impl App {
    pub(super) fn open_listening_tools(&mut self) {
        self.context_menu = None;
        self.catalog.sidebar = false;
        self.ui.overlay = Overlay::ListeningTools;
        self.ui.listening.selected = self.ui.listening.selected.min(ListeningTool::ALL.len() - 1);
        self.status = ListeningTool::ALL[self.ui.listening.selected]
            .detail()
            .into();
    }

    pub(super) fn listening_key(
        &mut self,
        code: KeyCode,
        tasks: &mut Tasks,
        tx: &mpsc::UnboundedSender<Command>,
    ) {
        match code {
            KeyCode::Esc | KeyCode::F(6) => self.ui.close(Overlay::ListeningTools),
            KeyCode::Up | KeyCode::Char('k') => {
                self.ui.listening.selected = self.ui.listening.selected.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.ui.listening.selected =
                    (self.ui.listening.selected + 1).min(ListeningTool::ALL.len() - 1)
            }
            KeyCode::Enter => {
                self.apply_listening_tool(
                    ListeningTool::ALL[self.ui.listening.selected],
                    tasks,
                    tx,
                );
                return;
            }
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char(' ') => self.media_action(MediaAction::Toggle, tx),
            _ => return,
        }
        if self.ui.overlay == Overlay::ListeningTools {
            self.status = ListeningTool::ALL[self.ui.listening.selected]
                .detail()
                .into();
        }
    }

    pub(super) fn apply_listening_tool(
        &mut self,
        tool: ListeningTool,
        tasks: &mut Tasks,
        tx: &mpsc::UnboundedSender<Command>,
    ) {
        if let Some(reason) = tool.unavailable(self) {
            self.status = reason.into();
            return;
        }
        match tool {
            ListeningTool::Sleep15
            | ListeningTool::Sleep30
            | ListeningTool::Sleep45
            | ListeningTool::Sleep60 => {
                let minutes = match tool {
                    ListeningTool::Sleep15 => 15,
                    ListeningTool::Sleep30 => 30,
                    ListeningTool::Sleep45 => 45,
                    _ => 60,
                };
                self.ui.listening.sleep =
                    SleepTimer::Deadline(Instant::now() + Duration::from_secs(minutes * 60));
                self.status =
                    format!("Sleep timer set for {minutes} minutes. F6 changes or cancels it.");
            }
            ListeningTool::StopAfterTrack => {
                self.ui.listening.sleep = SleepTimer::EndOfTrack(self.generation);
                self.status = "Will stop after the current track. F6 cancels; changing tracks cancels this timer.".into();
            }
            ListeningTool::CancelSleep => {
                self.ui.listening.sleep = SleepTimer::Off;
                self.status = "Sleep timer cancelled; playback unchanged.".into();
            }
            ListeningTool::RemovePlayed
            | ListeningTool::RemoveDuplicates
            | ListeningTool::RemoveUnavailable => self.clean_queue(tool, tasks, tx),
            ListeningTool::ConnectSpotify
            | ListeningTool::ConnectGoogle
            | ListeningTool::UpdatePlayback => {
                self.session_request = match tool {
                    ListeningTool::ConnectSpotify => crate::model::SessionRequest::ConnectSpotify,
                    ListeningTool::ConnectGoogle => crate::model::SessionRequest::ConnectGoogle,
                    _ => crate::model::SessionRequest::RepairPlayback,
                };
                self.quit = true;
            }
        }
        self.ui.close(Overlay::ListeningTools);
    }

    pub(super) fn check_sleep(
        &mut self,
        now: Instant,
        tx: &mpsc::UnboundedSender<Command>,
    ) -> bool {
        match self.ui.listening.sleep {
            SleepTimer::Deadline(at) if now >= at => {
                self.ui.listening.sleep = SleepTimer::Off;
                self.interpolate_position();
                self.stop(tx);
                self.status =
                    "Sleep timer finished; playback paused. Space resumes from this position."
                        .into();
                true
            }
            SleepTimer::EndOfTrack(generation) if generation != self.generation => {
                self.ui.listening.sleep = SleepTimer::Off;
                true
            }
            _ => false,
        }
    }

    pub(super) fn finish_track_sleep(&mut self, tx: &mpsc::UnboundedSender<Command>) -> bool {
        if self.ui.listening.sleep != SleepTimer::EndOfTrack(self.generation) {
            return false;
        }
        self.ui.listening.sleep = SleepTimer::Off;
        self.stop(tx);
        self.queue.position_ms = 0;
        self.status =
            "Sleep timer finished at track end. Space replays this track; n chooses the next."
                .into();
        true
    }

    fn clean_queue(
        &mut self,
        option: ListeningTool,
        tasks: &mut Tasks,
        tx: &mpsc::UnboundedSender<Command>,
    ) {
        let current = self.queue.cursor;
        let mut seen = HashSet::new();
        if let Some(id) = self.queue.current() {
            seen.insert(id.to_owned());
        }
        let removed: HashSet<usize> = self
            .queue
            .order
            .iter()
            .enumerate()
            .filter_map(|(at, &original)| {
                if current == Some(at) {
                    return None;
                }
                let id = &self.queue.ids[original];
                let remove = match option {
                    ListeningTool::RemovePlayed => current.is_some_and(|cursor| at < cursor),
                    ListeningTool::RemoveDuplicates => {
                        current.is_none_or(|cursor| at > cursor) && !seen.insert(id.clone())
                    }
                    ListeningTool::RemoveUnavailable => {
                        self.cache.get(id).is_some_and(|track| !track.playable)
                    }
                    _ => false,
                };
                remove.then_some(original)
            })
            .collect();
        if removed.is_empty() {
            self.status = "No matching entries to remove; queue unchanged.".into();
            return;
        }
        self.remember_queue();
        let count = self.queue.remove_originals(&removed);
        tasks.cancel_smart_shuffle();
        self.check_preload(tx);
        self.ui.render.borrow_mut().queue_scroll = 0;
        self.ui.render.borrow_mut().queue_filter_metadata_start = None;
        self.status =
            format!("Removed {count} queue entries; current track preserved. Press u to undo.");
    }
}
