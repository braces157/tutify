use super::*;

/// Application intent, independent of the key or menu that invoked it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    PlaySelected,
    EnqueueSelected,
    PlayNext,
    StartRadio,
    ClearQueue,
    MoveUp,
    MoveDown,
    RemoveSelected,
    Undo,
}

pub(super) fn apply(
    app: &mut App,
    action: Action,
    tasks: &mut Tasks,
    tx: &mpsc::UnboundedSender<Command>,
) {
    match action {
        Action::Undo => perform_undo(app, tasks, tx),
        Action::PlaySelected if app.catalog.view != View::Help => {
            if let Rows::Playlists(rows) = &app.catalog.rows {
                if app.catalog.view == View::Playlists {
                    let actual_idx = if app.is_filtered() {
                        app.filtered_indices().get(app.catalog.selected).copied()
                    } else {
                        Some(app.catalog.selected)
                    };
                    if let Some(idx) = actual_idx {
                        if let Some(p) = rows.get(idx).cloned() {
                            app.catalog.browse = Browse::Playlist(p.id);
                            app.catalog.title = p.name;
                            app.reset_rows();
                            app.catalog.selected = 0;
                            app.catalog.filter.clear();
                            app.catalog.filtering = false;
                            tasks.request(app, 0);
                        }
                    }
                    return;
                }
            }
            if let Some(track) = app.selected_track() {
                if !track.playable {
                    app.status = "This track is unavailable for your account or region.".into();
                    return;
                }
                if app.catalog.view == View::Search {
                    tasks.start_radio(app, track, tx);
                    return;
                }
                if app.catalog.view == View::Queue {
                    app.queue.select(app.queue.selected);
                } else if let Rows::Tracks(tracks) = &app.catalog.rows {
                    let (track_ids, index) = if app.is_filtered() {
                        let filtered = app.filtered_indices();
                        let playable_filtered: Vec<(usize, &Track)> = filtered
                            .iter()
                            .filter_map(|&i| tracks.get(i).map(|t| (i, t)))
                            .filter(|(_, t)| t.playable)
                            .collect();
                        let play_idx = playable_filtered
                            .iter()
                            .position(|(orig_idx, _)| {
                                filtered.get(app.catalog.selected) == Some(orig_idx)
                            })
                            .unwrap_or(0);
                        let ids: Vec<String> = playable_filtered
                            .into_iter()
                            .map(|(_, t)| t.id.clone())
                            .collect();
                        (ids, play_idx)
                    } else {
                        let index = tracks[..app.catalog.selected.min(tracks.len())]
                            .iter()
                            .filter(|t| t.playable)
                            .count();
                        let ids: Vec<String> = tracks
                            .iter()
                            .filter(|t| t.playable)
                            .map(|t| t.id.clone())
                            .collect();
                        (ids, index)
                    };
                    if track_ids.len() > crate::queue::MAX_TRACKS {
                        app.status = "This list exceeds the 100,000-track queue limit; filter it before playing.".into();
                        return;
                    }
                    app.remember_queue();
                    app.queue.replace(track_ids, index, app.config.shuffle);
                }
                app.cache.insert(track.id.clone(), track);
                app.load(tx);
            }
        }
        Action::EnqueueSelected if app.catalog.view != View::Help => {
            if let Rows::Playlists(playlists) = &app.catalog.rows {
                if app.catalog.view == View::Playlists {
                    let actual_idx = if app.is_filtered() {
                        app.filtered_indices().get(app.catalog.selected).copied()
                    } else {
                        Some(app.catalog.selected)
                    };
                    if let Some(idx) = actual_idx {
                        if let Some(p) = playlists.get(idx).cloned() {
                            tasks.enqueue_playlist(app, p.id, p.name);
                            return;
                        }
                    }
                }
            }
            if let Some(track) = app.selected_track() {
                if track.playable {
                    if app.queue.ids.len() < crate::queue::MAX_TRACKS {
                        app.remember_queue();
                    }
                    app.status = if app.enqueue_manual(track.id) {
                        format!("Added {} to queue", track.name)
                    } else {
                        "Queue limit reached (100,000 tracks).".into()
                    };
                } else {
                    app.status = "Unavailable track cannot be queued.".into();
                }
            }
        }
        Action::PlayNext if app.catalog.view != View::Help => {
            if let Some(track) = app.selected_track() {
                if track.playable {
                    if app.queue.ids.len() < crate::queue::MAX_TRACKS {
                        app.remember_queue();
                    }
                    app.status = if app.queue.insert_next(track.id) {
                        format!("Playing next: {}", track.name)
                    } else {
                        "Queue limit reached (100,000 tracks).".into()
                    };
                } else {
                    app.status = "Unavailable track cannot be queued.".into();
                }
            }
        }
        Action::StartRadio if app.catalog.view != View::Help => {
            if let Some(track) = app.selected_track() {
                if track.artists.is_empty() || track.name.is_empty() {
                    app.status =
                        "Wait for track metadata before starting Radio; F5 retries metadata."
                            .into();
                    return;
                }
                if track.playable {
                    tasks.start_radio(app, track, tx);
                } else {
                    app.status = "Unavailable track cannot start Radio.".into();
                }
            }
        }
        Action::ClearQueue if app.catalog.view == View::Queue => {
            if !app.queue.ids.is_empty() {
                app.remember_queue();
            }
            if app.queue.clear() {
                app.stop(tx);
            }
            app.status = "Queue cleared. Press u to undo.".into();
        }
        Action::MoveUp if app.catalog.view == View::Queue && app.queue.selected > 0 => {
            let from = app.queue.selected;
            let to = from - 1;
            app.remember_queue();
            app.queue.move_item(from, to);
            app.status = "Moved track up in queue".into();
        }
        Action::MoveDown
            if app.catalog.view == View::Queue
                && !app.queue.ids.is_empty()
                && app.queue.selected + 1 < app.queue.ids.len() =>
        {
            let from = app.queue.selected;
            let to = from + 1;
            app.remember_queue();
            app.queue.move_item(from, to);
            app.status = "Moved track down in queue".into();
        }
        Action::RemoveSelected if app.catalog.view == View::Queue => {
            if app.queue.selected < app.queue.order.len() {
                app.remember_queue();
            }
            if app.queue.remove(app.queue.selected) {
                app.stop(tx);
            }
            app.status = "Queue item removed. Press u to undo.".into();
        }
        _ => (),
    }
}
