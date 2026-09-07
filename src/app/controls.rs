use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum Seek {
    Start,
    End,
    Relative(i32),
    Position(u32),
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Control {
    Media(MediaAction),
    Seek(Seek),
    Volume(i16),
    Mute,
}

impl App {
    /// Shared by normal input, overlays, mouse controls, and media buttons.
    pub(super) fn control(&mut self, control: Control, tx: &mpsc::UnboundedSender<Command>) {
        match control {
            Control::Media(action) => self.media_action(action, tx),
            Control::Seek(seek) => {
                let Some(track) = self.current_track() else {
                    self.status = "Choose a track before seeking.".into();
                    return;
                };
                if track.duration_ms == 0 {
                    self.status = "Track duration is not available yet.".into();
                    return;
                }
                let position = match seek {
                    Seek::Start => 0,
                    Seek::End => track.duration_ms - 1,
                    Seek::Relative(delta) => self.queue.position_ms.saturating_add_signed(delta),
                    Seek::Position(position) => position,
                };
                self.queue.position_ms = position.min(track.duration_ms - 1);
                self.anchor_position();
                if self.loaded {
                    self.send(tx, Command::Seek(self.queue.position_ms));
                }
                self.status = format!(
                    "Seeked to {}. Left/Right seek 10s | Home/End jump",
                    format_time(self.queue.position_ms)
                );
            }
            Control::Volume(delta) => {
                self.config.volume = (i16::from(self.config.volume) + delta).clamp(0, 100) as u8;
                if delta > 0 || self.config.volume > 0 {
                    self.muted_volume = None;
                }
                self.send(tx, Command::Volume(self.config.volume));
                self.status = format!(
                    "Volume {}%{}",
                    self.config.volume,
                    if delta.abs() == 1 { " (fine)" } else { "" }
                );
            }
            Control::Mute => {
                if self.config.volume == 0 {
                    self.config.volume = self.muted_volume.take().unwrap_or(50);
                } else {
                    self.muted_volume = Some(self.config.volume);
                    self.config.volume = 0;
                }
                self.send(tx, Command::Volume(self.config.volume));
                self.status = if self.config.volume == 0 {
                    "Muted. Press m to restore volume".into()
                } else {
                    format!("Volume {}%", self.config.volume)
                };
            }
        }
    }
}
