use super::*;

pub(super) fn key(
    app: &mut App,
    key: KeyEvent,
    tasks: &mut Tasks,
    tx: &mpsc::UnboundedSender<Command>,
) {
    if key.kind == KeyEventKind::Release {
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.quit = true;
        return;
    }
    if app.ui.overlay == Overlay::Stats {
        stats_key(app, key, tx);
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('z') {
        perform_undo(app, tasks, tx);
        return;
    }
    if matches!(key.code, KeyCode::F(2) | KeyCode::F(3)) {
        choose_search(
            app,
            if key.code == KeyCode::F(2) {
                SearchScope::Spotify
            } else {
                SearchScope::Library
            },
            tasks,
        );
        return;
    }
    if let Some(menu) = &mut app.context_menu {
        match key.code {
            KeyCode::Esc => app.context_menu = None,
            KeyCode::Down => menu.selected = (menu.selected + 1) % menu.actions.len(),
            KeyCode::Up => {
                menu.selected = (menu.selected + menu.actions.len() - 1) % menu.actions.len()
            }
            KeyCode::Enter => {
                let action = menu.selected;
                activate_menu(app, action, tasks, tx);
            }
            KeyCode::Char('q') => app.quit = true,
            _ => (),
        }
        return;
    }
    if app.catalog.editing {
        match key.code {
            KeyCode::Esc => app.catalog.editing = false,
            KeyCode::Enter => {
                app.catalog.editing = false;
                app.catalog.browse = Browse::Search(app.catalog.query.trim().into());
                app.catalog.selected = 0;
                app.reset_rows();
                tasks.request(app, 0);
            }
            KeyCode::Backspace => {
                app.catalog.query.pop();
            }
            KeyCode::Char(c) if !c.is_control() && app.catalog.query.len() < 500 => {
                app.catalog.query.push(c)
            }
            _ => (),
        }
        return;
    }
    if app.catalog.filtering {
        match key.code {
            KeyCode::Esc => {
                app.catalog.filter.clear();
                app.catalog.filtering = false;
                app.catalog.selected = 0;
                return;
            }
            KeyCode::Enter => {
                app.catalog.filtering = false;
            }
            KeyCode::Down => {
                app.catalog.filtering = false;
                if app.len() > 1 {
                    app.catalog.selected = 1;
                }
                return;
            }
            KeyCode::Tab => {
                app.catalog.filtering = false;
                return;
            }
            KeyCode::Backspace => {
                app.catalog.filter.pop();
                app.catalog.selected = 0;
                return;
            }
            KeyCode::Char(c) if !c.is_control() && app.catalog.filter.len() < 100 => {
                app.catalog.filter.push(c);
                app.catalog.selected = 0;
                return;
            }
            _ => return,
        }
    }
    if app.ui.overlay == Overlay::Lyrics
        && app
            .lyrics
            .content
            .as_ref()
            .is_some_and(|l| l.lines.is_empty())
    {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                app.lyrics.scroll = (app.lyrics.scroll + 1)
                    .min(app.ui.render.borrow().lyrics_length.saturating_sub(1));
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.lyrics.scroll = app.lyrics.scroll.saturating_sub(1);
                return;
            }
            KeyCode::PageDown => {
                app.lyrics.scroll = (app.lyrics.scroll + 10)
                    .min(app.ui.render.borrow().lyrics_length.saturating_sub(1));
                return;
            }
            KeyCode::PageUp => {
                app.lyrics.scroll = app.lyrics.scroll.saturating_sub(10);
                return;
            }
            _ => (),
        }
    }
    match key.code {
        KeyCode::Char('u') => perform_undo(app, tasks, tx),
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Esc
            if app.catalog.view == View::Search
                && app.catalog.search_scope == SearchScope::Library
                && app.catalog.busy =>
        {
            if let Some(scan) = tasks.browse.take() {
                scan.abort();
            }
            app.catalog.request += 1;
            app.catalog.busy = false;
            app.catalog.title = "Saved library — partial results".into();
            app.status = format!(
                "Library search cancelled after {} tracks. Partial results retained; F5 restarts.",
                app.catalog.library_scanned
            );
        }
        KeyCode::Esc if app.ui.overlay == Overlay::Lyrics => {
            app.ui.close(Overlay::Lyrics);
            app.status = "Exited lyrics view".into();
        }
        KeyCode::Esc if app.ui.overlay == Overlay::Visualizer => {
            app.ui.close(Overlay::Visualizer);
            app.status = "Exited visualizer".into();
        }
        KeyCode::Esc if !app.catalog.filter.is_empty() => {
            app.catalog.filter.clear();
            app.catalog.filtering = false;
            app.catalog.selected = 0;
        }
        KeyCode::Esc if app.catalog.view != View::Help => app.quit = true,
        KeyCode::Esc => tasks.view(app, View::Search),
        KeyCode::Tab | KeyCode::BackTab => {
            app.catalog.sidebar = !app.catalog.sidebar;
            app.catalog.nav = app.catalog.view.index();
        }
        KeyCode::Char('?') | KeyCode::F(1) => tasks.view(app, View::Help),
        KeyCode::Char(c @ '1'..='5') => tasks.view(app, View::ALL[c as usize - '1' as usize]),
        KeyCode::Char('/') => {
            if matches!(app.catalog.view, View::Liked | View::Playlists) {
                app.catalog.filter.clear();
                app.catalog.filtering = true;
                app.catalog.selected = 0;
            } else {
                tasks.view(app, View::Search);
                app.catalog.editing = true;
            }
        }
        KeyCode::Char('f') if matches!(app.catalog.view, View::Liked | View::Playlists) => {
            app.catalog.filter.clear();
            app.catalog.filtering = true;
            app.catalog.selected = 0;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.catalog.sidebar {
                app.catalog.nav = app.catalog.nav.saturating_sub(1);
            } else if app.catalog.view == View::Queue {
                app.queue.selected = app.queue.selected.saturating_sub(1);
            } else {
                app.catalog.selected = app.catalog.selected.saturating_sub(1);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.catalog.sidebar {
                app.catalog.nav = (app.catalog.nav + 1).min(4);
            } else if app.catalog.view == View::Queue {
                app.queue.selected =
                    (app.queue.selected + 1).min(app.queue.ids.len().saturating_sub(1));
            } else {
                let at = (app.catalog.selected + 1).min(app.len().saturating_sub(1));
                app.catalog.selected = at;
                if app.ui.overlay != Overlay::Stats
                    && !app.catalog.busy
                    && !app.is_filtered()
                    && at + 5 >= app.len()
                {
                    if let Some(offset) = app.catalog.next {
                        tasks.request(app, offset);
                    }
                }
            }
        }
        KeyCode::PageUp => {
            if app.catalog.sidebar {
                app.catalog.nav = 0;
            } else if app.catalog.view == View::Queue {
                app.queue.selected = app.queue.selected.saturating_sub(15);
            } else {
                app.catalog.selected = app.catalog.selected.saturating_sub(15);
            }
        }
        KeyCode::PageDown | KeyCode::Char('>') if !matches!(app.catalog.view, View::Help) => {
            if app.catalog.sidebar {
                app.catalog.nav = 4;
            } else if app.catalog.view == View::Queue {
                app.queue.selected =
                    (app.queue.selected + 15).min(app.queue.ids.len().saturating_sub(1));
            } else {
                app.catalog.selected = (app.catalog.selected + 15).min(app.len().saturating_sub(1));
                if !app.catalog.busy && !app.is_filtered() {
                    if let Some(offset) = app.catalog.next {
                        tasks.request(app, offset);
                    }
                }
            }
        }
        KeyCode::F(5) => {
            tasks.retry_metadata(app);
            if matches!(
                app.catalog.view,
                View::Search | View::Playlists | View::Liked
            ) {
                tasks.request(app, 0);
            }
        }
        KeyCode::Backspace if matches!(app.catalog.browse, Browse::Playlist(_)) => {
            tasks.view(app, View::Playlists)
        }
        KeyCode::Enter if app.catalog.sidebar => tasks.view(app, View::ALL[app.catalog.nav]),
        KeyCode::Enter if app.catalog.view != View::Help => {
            actions::apply(app, Action::PlaySelected, tasks, tx)
        }
        code if playback_control(code).is_some() => {
            app.control(playback_control(code).unwrap(), tx);
        }
        KeyCode::Char('s') => {
            app.remember_queue();
            app.config.shuffle = !app.config.shuffle;
            app.queue.set_shuffle(app.config.shuffle);
            app.status = format!("Shuffle {}", if app.config.shuffle { "on" } else { "off" });
        }
        KeyCode::Char('r') => {
            app.config.repeat = app.config.repeat.cycle();
            app.status = format!("Repeat: {:?}", app.config.repeat);
        }
        KeyCode::Char('t') => {
            let current = ui::Theme::from_str(&app.config.theme);
            let next = current.next();
            app.config.theme = next.as_str().to_string();
            app.status = format!("Theme: {}", next.name());
        }
        KeyCode::Char('l') => {
            app.ui.toggle(Overlay::Lyrics);
            if app.ui.overlay == Overlay::Lyrics {
                app.status = "Lyrics active (press l or Esc to exit)".into();
            } else {
                app.status = "Exited lyrics".into();
            }
        }
        KeyCode::Char('v') => {
            app.ui.toggle(Overlay::Visualizer);
            if app.ui.overlay == Overlay::Visualizer {
                app.status = "Retro visualizer active (press v or Esc to exit)".into();
            } else {
                app.status = "Exited visualizer".into();
            }
        }
        KeyCode::Char('S') => {
            app.ui.overlay = Overlay::Stats;
            app.catalog.sidebar = false;
            app.ui.stats.get_mut().selected = 0;
            app.ui.render.borrow_mut().stats_scroll = 0;
            app.ui.stats.get_mut().editing = false;
            app.stats.refresh_metadata(&app.cache);
            app.status = "Song statistics (press S or Esc to exit)".into();
        }
        KeyCode::Char('a') if app.catalog.view != View::Help => {
            actions::apply(app, Action::EnqueueSelected, tasks, tx)
        }
        KeyCode::Char('A') if app.catalog.view != View::Help => {
            actions::apply(app, Action::PlayNext, tasks, tx)
        }
        KeyCode::Char('R') if app.catalog.view != View::Help => {
            actions::apply(app, Action::StartRadio, tasks, tx)
        }
        KeyCode::Char('C') if app.catalog.view == View::Queue => {
            actions::apply(app, Action::ClearQueue, tasks, tx)
        }
        KeyCode::Char('K') if app.catalog.view == View::Queue && app.queue.selected > 0 => {
            actions::apply(app, Action::MoveUp, tasks, tx)
        }
        KeyCode::Char('J')
            if app.catalog.view == View::Queue
                && !app.queue.ids.is_empty()
                && app.queue.selected + 1 < app.queue.ids.len() =>
        {
            actions::apply(app, Action::MoveDown, tasks, tx)
        }
        KeyCode::Char('.') | KeyCode::Char('c') if app.catalog.view == View::Queue => {
            if let Some(c) = app.queue.cursor {
                app.queue.selected = c;
                app.status = "Jumped to currently playing track.".into();
            }
        }
        KeyCode::Delete | KeyCode::Char('d') | KeyCode::Char('x')
            if app.catalog.view == View::Queue =>
        {
            actions::apply(app, Action::RemoveSelected, tasks, tx)
        }
        _ => (),
    }
}

fn playback_control(code: KeyCode) -> Option<Control> {
    Some(match code {
        KeyCode::Char(' ') => Control::Media(MediaAction::Toggle),
        KeyCode::Char('n') => Control::Media(MediaAction::Next),
        KeyCode::Char('p') => Control::Media(MediaAction::Previous),
        KeyCode::Home => Control::Seek(Seek::Start),
        KeyCode::End => Control::Seek(Seek::End),
        KeyCode::Left => Control::Seek(Seek::Relative(-10_000)),
        KeyCode::Right => Control::Seek(Seek::Relative(10_000)),
        KeyCode::Char('+') | KeyCode::Char('=') => Control::Volume(5),
        KeyCode::Char('-') => Control::Volume(-5),
        KeyCode::Char(']') => Control::Volume(1),
        KeyCode::Char('[') => Control::Volume(-1),
        KeyCode::Char('m') => Control::Mute,
        _ => return None,
    })
}

fn stats_key(app: &mut App, key: KeyEvent, tx: &mpsc::UnboundedSender<Command>) {
    let view = app.ui.stats.get_mut();
    if view.editing {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => view.editing = false,
            KeyCode::Backspace => {
                view.query.pop();
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && !c.is_control()
                    && view.query.chars().count() < 100 =>
            {
                view.query.push(c)
            }
            _ => {}
        }
        app.ui.stats.get_mut().selected = 0;
        app.ui.render.borrow_mut().stats_scroll = 0;
        return;
    }
    match key.code {
        KeyCode::Char('/') => app.ui.stats.get_mut().editing = true,
        KeyCode::Tab => {
            let view = app.ui.stats.get_mut();
            view.sort = view.sort.next();
            app.ui.stats.get_mut().selected = 0;
            app.ui.render.borrow_mut().stats_scroll = 0;
        }
        KeyCode::Esc if !app.ui.stats.borrow().query.is_empty() => {
            app.ui.stats.get_mut().query.clear();
            app.ui.stats.get_mut().selected = 0;
            app.ui.render.borrow_mut().stats_scroll = 0;
        }
        KeyCode::Char('S') | KeyCode::Esc => {
            app.ui.close(Overlay::Stats);
            app.status = "Exited song statistics".into();
        }
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Up | KeyCode::Char('k') => {
            app.ui.stats.get_mut().selected = app.ui.stats.get_mut().selected.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let max_idx = app.len().saturating_sub(1);
            app.ui.stats.get_mut().selected = (app.ui.stats.get_mut().selected + 1).min(max_idx);
        }
        KeyCode::PageUp => {
            app.ui.stats.get_mut().selected = app.ui.stats.get_mut().selected.saturating_sub(15);
        }
        KeyCode::PageDown | KeyCode::Char('>') => {
            let max_idx = app.len().saturating_sub(1);
            app.ui.stats.get_mut().selected = (app.ui.stats.get_mut().selected + 15).min(max_idx);
        }
        code if playback_control(code).is_some() => {
            app.control(playback_control(code).unwrap(), tx);
        }
        _ => (),
    }
}
