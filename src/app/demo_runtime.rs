use super::*;

pub async fn run_demo() -> Result<()> {
    let mut app = crate::demo::app();
    let (background_tx, mut background_rx) = mpsc::unbounded_channel();
    let mut tasks = Tasks::demo(background_tx)?;
    let (commands, mut command_rx) = mpsc::unbounded_channel();
    let mut playback = DemoPlayback::default();
    let mut terminal = ui::TerminalGuard::enter()?;
    let mut keys = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tasks.sync_queue_epoch(app.queue.epoch);
        tasks.refill_radio(&app);
        tasks.refill_smart_shuffle(&app);
        tasks.metadata(&app);
        terminal
            .terminal
            .draw(|frame| ui::draw(frame, &app, &mut app.ui.render.borrow_mut()))?;
        tokio::select! {
            input = keys.next() => match input {
                Some(Ok(event)) => {
                    route_input(&mut app, event, &mut tasks, &commands);
                }
                Some(Err(error)) => return Err(error.into()),
                None => break,
            },
            Some(event) = background_rx.recv() => {
                background(&mut app, &mut tasks, event);
            }
            Some(command) = command_rx.recv() => playback.command(&mut app, command, &commands),
            _ = tick.tick() => {
                tasks.update_lyrics(&mut app);
                playback.tick(&mut app, &commands);
            }
            _ = tokio::signal::ctrl_c() => break,
        }
        if app.quit {
            break;
        }
    }
    drop(terminal);
    Ok(())
}

#[derive(Default)]
struct DemoPlayback {
    active_generation: Option<u64>,
    playing: bool,
}

impl DemoPlayback {
    fn command(&mut self, app: &mut App, command: Command, tx: &mpsc::UnboundedSender<Command>) {
        match command {
            Command::Load {
                position_ms,
                generation,
                ..
            } if generation == app.generation => {
                self.active_generation = Some(generation);
                self.playing = true;
                app.playback_event(
                    Event::Playing {
                        generation,
                        position_ms,
                    },
                    tx,
                );
            }
            Command::Load { .. } => (),
            Command::Preload { .. } => (),
            Command::Seek(position_ms) if self.active_generation == Some(app.generation) => {
                // Production seeking reports a position update and preserves the
                // current play/pause intent.
                app.playback_event(
                    Event::Position {
                        generation: app.generation,
                        position_ms,
                    },
                    tx,
                );
            }
            Command::Seek(_) => (),
            Command::Pause if self.active_generation == Some(app.generation) => {
                self.playing = false;
                app.playback_event(
                    Event::Paused {
                        generation: app.generation,
                        position_ms: app.queue.position_ms,
                    },
                    tx,
                );
            }
            Command::Pause => (),
            Command::Resume if self.active_generation == Some(app.generation) => {
                self.playing = true;
                app.playback_event(
                    Event::Playing {
                        generation: app.generation,
                        position_ms: app.queue.position_ms,
                    },
                    tx,
                );
            }
            Command::Resume => (),
            Command::Volume(volume) => app.playback_event(Event::Volume(volume), tx),
            Command::Stop => {
                self.active_generation = None;
                self.playing = false;
            }
            #[cfg(test)]
            Command::SimulateDisconnect => (),
        }
    }

    fn tick(&mut self, app: &mut App, tx: &mpsc::UnboundedSender<Command>) {
        if !self.playing || self.active_generation != Some(app.generation) {
            return;
        }
        let next = app.queue.position_ms.saturating_add(1_000);
        let duration = app
            .current_track()
            .map_or(u32::MAX, |track| track.duration_ms);
        if next >= duration {
            app.playback_event(Event::Completed(app.generation), tx);
        } else {
            app.playback_event(
                Event::Position {
                    generation: app.generation,
                    position_ms: next,
                },
                tx,
            );
            app.status =
                "DEMO • SIMULATED PLAYBACK • no Spotify, network, Discord, or audio device".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_seek_stays_paused_and_stopped_generation_rejects_late_seek() {
        let mut app = crate::demo::app();
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut playback = DemoPlayback::default();
        app.generation = 1;
        let id = app.queue.ids[0].clone();
        playback.command(
            &mut app,
            Command::Load {
                id,
                position_ms: 0,
                generation: 1,
            },
            &tx,
        );
        playback.command(&mut app, Command::Pause, &tx);
        playback.command(&mut app, Command::Seek(10_000), &tx);
        assert_eq!(app.state, State::Paused);
        assert_eq!(app.queue.position_ms, 10_000);
        playback.tick(&mut app, &tx);
        assert_eq!(app.queue.position_ms, 10_000);
        playback.command(&mut app, Command::Resume, &tx);
        playback.tick(&mut app, &tx);
        assert_eq!(app.queue.position_ms, 11_000);
        playback.command(&mut app, Command::Stop, &tx);
        app.generation += 1;
        playback.command(&mut app, Command::Seek(25_000), &tx);
        assert_eq!(app.queue.position_ms, 11_000);
    }
}
