use super::*;

pub const TOOL_LABELS: [&str; 9] = [
    "Sleep in 15 minutes",
    "Sleep in 30 minutes",
    "Sleep in 45 minutes",
    "Sleep in 60 minutes",
    "Stop after current track",
    "Cancel sleep timer",
    "Remove played queue entries",
    "Remove upcoming duplicates",
    "Remove unavailable tracks",
];

pub const TOOL_DETAILS: [&str; 9] = [
    "Pause in 15 minutes, keeping your queue and position. Enter applies; Esc closes.",
    "Pause in 30 minutes, keeping your queue and position. Enter applies; Esc closes.",
    "Pause in 45 minutes, keeping your queue and position. Enter applies; Esc closes.",
    "Pause in 60 minutes, keeping your queue and position. Enter applies; Esc closes.",
    "Finish the loaded track, then stop even with Repeat enabled. Changing tracks cancels it.",
    "Cancel the timer without changing playback or volume.",
    "Remove entries before the current track in the full queue. Current track stays; u undoes.",
    "Keep one occurrence of each upcoming track ID, excluding the current track. u undoes.",
    "Remove known unavailable entries from the full queue. Current and unknown tracks stay; u undoes.",
];

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
        self.ui.listening.selected = self.ui.listening.selected.min(TOOL_LABELS.len() - 1);
        self.status = TOOL_DETAILS[self.ui.listening.selected].into();
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
                    (self.ui.listening.selected + 1).min(TOOL_LABELS.len() - 1)
            }
            KeyCode::Enter => {
                self.apply_listening_tool(tasks, tx);
                return;
            }
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char(' ') => self.media_action(MediaAction::Toggle, tx),
            _ => return,
        }
        if self.ui.overlay == Overlay::ListeningTools {
            self.status = TOOL_DETAILS[self.ui.listening.selected].into();
        }
    }

    fn apply_listening_tool(&mut self, tasks: &mut Tasks, tx: &mpsc::UnboundedSender<Command>) {
        match self.ui.listening.selected {
            at @ 0..=3 => {
                let minutes = [15, 30, 45, 60][at];
                self.ui.listening.sleep =
                    SleepTimer::Deadline(Instant::now() + Duration::from_secs(minutes * 60));
                self.status =
                    format!("Sleep timer set for {minutes} minutes. F6 changes or cancels it.");
            }
            4 if self.loaded && self.queue.current().is_some() && self.state != State::Failed => {
                self.ui.listening.sleep = SleepTimer::EndOfTrack(self.generation);
                self.status = "Will stop after the current track. F6 cancels; changing tracks cancels this timer.".into();
            }
            4 => {
                self.status = "Start a track before choosing stop after current track.".into();
                return;
            }
            5 => {
                self.ui.listening.sleep = SleepTimer::Off;
                self.status = "Sleep timer cancelled; playback unchanged.".into();
            }
            at @ 6..=8 => self.clean_queue(at, tasks, tx),
            _ => return,
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
        option: usize,
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
                    6 => current.is_some_and(|cursor| at < cursor),
                    7 => current.is_none_or(|cursor| at > cursor) && !seen.insert(id.clone()),
                    8 => self.cache.get(id).is_some_and(|track| !track.playable),
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
