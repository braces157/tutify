use super::*;

pub(super) fn format_time(ms: u32) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1_000 % 60)
}

pub(super) fn choose_search(app: &mut App, scope: SearchScope, tasks: &mut Tasks) {
    let scope = if app.config.source == crate::model::MusicSource::Youtube {
        if scope == SearchScope::Library && !app.config.youtube_connected {
            app.status =
                "Connect your Google music library first: quit (q), then run 'tuitify youtube login'."
                    .into();
            return;
        }
        if scope == SearchScope::Library {
            scope
        } else {
            SearchScope::Youtube
        }
    } else {
        scope
    };
    if app.is_filtered() {
        app.catalog.query = app.catalog.filter.clone();
    }
    app.catalog.history.clear();
    app.catalog.search_scope = scope;
    tasks.view(app, View::Search);
    app.context_menu = None;
    app.ui.overlay = Overlay::None;
    app.catalog.editing = app.catalog.query.trim().is_empty();
}
pub(super) fn perform_undo(app: &mut App, tasks: &mut Tasks, tx: &mpsc::UnboundedSender<Command>) {
    let existed = app.can_undo();
    app.undo_queue(tx);
    if existed {
        tasks.view(app, View::Queue);
        tasks.sync_queue_epoch(app.queue.epoch);
    }
}

pub(super) fn activate_menu(
    app: &mut App,
    action: usize,
    tasks: &mut Tasks,
    tx: &mpsc::UnboundedSender<Command>,
) {
    let Some(menu) = app.context_menu.take() else {
        return;
    };
    if app.ui.overlay == Overlay::Stats {
        return;
    }
    let revision = if menu.view == View::Queue {
        app.queue.revision
    } else {
        app.catalog.rows_revision
    };
    if app.catalog.view != menu.view
        || app.selection() != menu.row
        || revision != menu.revision
        || app.active_filter() != menu.filter
    {
        app.status = "List changed; right-click the track again.".into();
        return;
    }
    if let Some((_, action)) = menu.actions.get(action) {
        actions::apply(app, *action, tasks, tx);
    }
}

pub(super) fn mouse(
    app: &mut App,
    event: MouseEvent,
    tasks: &mut Tasks,
    tx: &mpsc::UnboundedSender<Command>,
) -> bool {
    if app.ui.overlay == Overlay::Diagnostics {
        let target = app
            .ui
            .render
            .borrow()
            .mouse_hits
            .iter()
            .rev()
            .find_map(|(area, target)| {
                area.contains((event.column, event.row).into())
                    .then_some(*target)
            });
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(MouseTarget::DiagnosticRow(index)) = target
                    && index < app.ui.diagnostics.history.records().len()
                {
                    app.ui.diagnostics.selected = index;
                }
                if let Some(MouseTarget::DiagnosticKey(code)) = target {
                    diagnostics_key(app, code, tasks);
                }
            }
            MouseEventKind::ScrollUp => diagnostics_key(app, KeyCode::Up, tasks),
            MouseEventKind::ScrollDown => diagnostics_key(app, KeyCode::Down, tasks),
            _ => (),
        }
        return true;
    }
    if app.ui.overlay == Overlay::LibraryCoverage {
        let target = app
            .ui
            .render
            .borrow()
            .mouse_hits
            .iter()
            .rev()
            .find_map(|(area, target)| {
                area.contains((event.column, event.row).into())
                    .then_some(*target)
            });
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => match target {
                Some(MouseTarget::CoverageRow(index))
                    if index < app.catalog.library_skipped.len() =>
                {
                    app.ui.coverage_selected = index;
                }
                Some(MouseTarget::CoverageClose) => app.ui.close(Overlay::LibraryCoverage),
                Some(MouseTarget::CoverageRecheck) => {
                    input::coverage_key(app, KeyCode::F(5), tasks)
                }
                _ => (),
            },
            MouseEventKind::ScrollUp => input::coverage_key(app, KeyCode::Up, tasks),
            MouseEventKind::ScrollDown => input::coverage_key(app, KeyCode::Down, tasks),
            _ => (),
        }
        return true;
    }
    if app.ui.overlay == Overlay::ListeningTools {
        let target = app
            .ui
            .render
            .borrow()
            .mouse_hits
            .iter()
            .rev()
            .find_map(|(area, target)| {
                area.contains((event.column, event.row).into())
                    .then_some(*target)
            });
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => match target {
                Some(MouseTarget::ListeningRow(index)) if index < listening::TOOL_LABELS.len() => {
                    app.ui.listening.selected = index;
                    app.status = listening::TOOL_DETAILS[index].into();
                }
                Some(MouseTarget::ListeningApply) => app.listening_key(KeyCode::Enter, tasks, tx),
                Some(MouseTarget::ListeningClose) => app.listening_key(KeyCode::Esc, tasks, tx),
                _ => (),
            },
            MouseEventKind::ScrollUp => app.listening_key(KeyCode::Up, tasks, tx),
            MouseEventKind::ScrollDown => app.listening_key(KeyCode::Down, tasks, tx),
            _ => (),
        }
        return true;
    }
    if app.ui.overlay == Overlay::MixBuilder {
        // Only the overlay's own visible preview controls can receive clicks.
        // Applying a mix remains an explicit keyboard action.
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            let target =
                app.ui
                    .render
                    .borrow()
                    .mouse_hits
                    .iter()
                    .rev()
                    .find_map(|(area, target)| {
                        area.contains((event.column, event.row).into())
                            .then_some(*target)
                    });
            match target {
                Some(MouseTarget::MixRow(index))
                    if !app.mix.detail
                        && !app.mix.naming
                        && index < app.mix.preview.entries.len() =>
                {
                    app.mix.selected = index;
                }
                Some(MouseTarget::MixKey(code))
                    if matches!(
                        code,
                        KeyCode::Esc
                            | KeyCode::Char(
                                'p' | 'g' | '3' | '4' | '6' | '[' | ']' | 'a' | 'w' | 'o' | '?'
                            )
                    ) =>
                {
                    key(app, KeyEvent::new(code, KeyModifiers::NONE), tasks, tx);
                }
                Some(MouseTarget::MixKey(KeyCode::Enter)) if app.mix.naming => {
                    key(
                        app,
                        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                        tasks,
                        tx,
                    );
                }
                _ => {}
            }
        } else if matches!(
            event.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let code = if event.kind == MouseEventKind::ScrollUp {
                KeyCode::Up
            } else {
                KeyCode::Down
            };
            for _ in 0..3 {
                key(app, KeyEvent::new(code, KeyModifiers::NONE), tasks, tx);
            }
        }
        return true;
    }
    if !matches!(
        event.kind,
        MouseEventKind::Down(MouseButton::Left | MouseButton::Right)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
    ) {
        return false;
    }
    let hit = app
        .ui
        .render
        .borrow()
        .mouse_hits
        .iter()
        .rev()
        .find(|(area, _)| area.contains((event.column, event.row).into()))
        .copied();
    if app.ui.overlay == Overlay::Stats {
        match event.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                app.ui.stats.get_mut().editing = false;
                let code = if event.kind == MouseEventKind::ScrollUp {
                    KeyCode::Up
                } else {
                    KeyCode::Down
                };
                for _ in 0..3 {
                    key(app, KeyEvent::new(code, KeyModifiers::NONE), tasks, tx);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => match hit.map(|(_, target)| target) {
                Some(MouseTarget::StatsRow(index)) => {
                    let max_index = app.len().saturating_sub(1);
                    let view = app.ui.stats.get_mut();
                    view.editing = false;
                    view.selected = index.min(max_index);
                }
                Some(MouseTarget::StatsSearch) => app.ui.stats.get_mut().editing = true,
                Some(MouseTarget::StatsSort) => {
                    key(
                        app,
                        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                        tasks,
                        tx,
                    );
                }
                Some(MouseTarget::StatsSearchDone) => {
                    key(
                        app,
                        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                        tasks,
                        tx,
                    );
                }
                Some(MouseTarget::StatsClear) => {
                    key(
                        app,
                        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                        tasks,
                        tx,
                    );
                }
                Some(MouseTarget::StatsClose) => {
                    app.ui.close(Overlay::Stats);
                    app.status = "Exited song statistics".into();
                }
                _ => {}
            },
            MouseEventKind::Down(MouseButton::Right) => {}
            _ => unreachable!("mouse event kind checked above"),
        }
        return true;
    }
    if app.context_menu.is_some() {
        if event.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some((_, MouseTarget::Menu(index))) = hit
        {
            activate_menu(app, index, tasks, tx);
            return true;
        }
        app.context_menu = None;
        return true;
    }
    let Some((area, target)) = hit else {
        return false;
    };
    if event.kind == MouseEventKind::ScrollUp || event.kind == MouseEventKind::ScrollDown {
        if app.ui.overlay == Overlay::Visualizer
            || (app.ui.overlay == Overlay::Lyrics
                && app
                    .lyrics
                    .content
                    .as_ref()
                    .is_none_or(|lyrics| !lyrics.lines.is_empty()))
        {
            return false;
        }
        if !matches!(
            target,
            MouseTarget::Catalog(_)
                | MouseTarget::Queue(_)
                | MouseTarget::CatalogScroll
                | MouseTarget::QueueScroll
        ) {
            return false;
        }
        if matches!(target, MouseTarget::Queue(_) | MouseTarget::QueueScroll)
            && app.catalog.view != View::Queue
        {
            app.clear_queue_filter();
            tasks.view(app, View::Queue);
            app.ui.overlay = Overlay::None;
        }
        app.catalog.editing = false;
        app.catalog.filtering = false;
        app.ui.queue.editing = false;
        app.catalog.sidebar = false;
        let code = if event.kind == MouseEventKind::ScrollUp {
            KeyCode::Up
        } else {
            KeyCode::Down
        };
        for _ in 0..3 {
            key(app, KeyEvent::new(code, KeyModifiers::NONE), tasks, tx);
        }
        return true;
    }
    let right = event.kind == MouseEventKind::Down(MouseButton::Right);
    match target {
        MouseTarget::LibraryCoverage if !right => {
            app.context_menu = None;
            app.catalog.editing = false;
            app.ui.overlay = Overlay::LibraryCoverage;
        }
        MouseTarget::ListeningTools if !right => app.open_listening_tools(),
        MouseTarget::SearchMode(scope) if !right => choose_search(app, scope, tasks),
        MouseTarget::Navigation(view) if !right => {
            app.catalog.history.clear();
            tasks.view(app, view);
            app.ui.overlay = Overlay::None;
        }
        MouseTarget::QueueScroll if right && app.can_undo() => {
            if app.catalog.view != View::Queue {
                app.clear_queue_filter();
                tasks.view(app, View::Queue);
            }
            app.catalog.sidebar = false;
            app.catalog.editing = false;
            app.catalog.filtering = false;
            app.context_menu = Some(ContextMenu {
                x: event.column,
                y: event.row,
                selected: 0,
                actions: vec![("Undo queue change", Action::Undo)],
                view: app.catalog.view,
                row: app.selection(),
                revision: app.queue.revision,
                filter: app.active_filter().to_owned(),
            });
        }
        MouseTarget::Prompt if !right => {
            app.catalog.sidebar = false;
            if app.catalog.view == View::Search {
                app.catalog.editing = true;
            } else {
                app.catalog.filtering = true;
            }
        }
        MouseTarget::QueueFilter if !right => app.start_queue_filter(),
        MouseTarget::QueueFilterClear if !right => {
            app.clear_queue_filter();
            app.status = "Queue filter cleared; all tracks visible.".into();
        }
        MouseTarget::Catalog(index) | MouseTarget::Queue(index) => {
            if matches!(target, MouseTarget::Queue(_)) {
                if app.catalog.view != View::Queue {
                    app.clear_queue_filter();
                    tasks.view(app, View::Queue);
                }
                app.ui.overlay = Overlay::None;
                app.queue.selected = index.min(app.queue.order.len().saturating_sub(1));
            } else {
                app.catalog.selected = index;
            }
            app.catalog.editing = false;
            app.catalog.filtering = false;
            app.ui.queue.editing = false;
            app.catalog.sidebar = false;
            if right {
                let playlist = app.catalog.view == View::Playlists
                    && matches!(app.catalog.rows, Rows::Playlists(_));
                let actions = if playlist {
                    vec![
                        ("Open playlist", Action::PlaySelected),
                        ("Add playlist to queue", Action::EnqueueSelected),
                    ]
                } else if app.catalog.view == View::Queue {
                    let mut queue_actions = vec![
                        ("Play", Action::PlaySelected),
                        ("Add to queue", Action::EnqueueSelected),
                        ("Play next", Action::PlayNext),
                        ("Remove from queue", Action::RemoveSelected),
                        ("View Album", Action::ViewAlbum),
                        ("View Artist", Action::ViewArtist),
                    ];
                    if app.can_undo() {
                        queue_actions.push(("Undo queue change", Action::Undo));
                    }
                    queue_actions
                } else {
                    vec![
                        ("Play", Action::PlaySelected),
                        ("Add to queue", Action::EnqueueSelected),
                        ("Play next", Action::PlayNext),
                        ("View Album", Action::ViewAlbum),
                        ("View Artist", Action::ViewArtist),
                    ]
                };
                app.context_menu = Some(ContextMenu {
                    x: event.column,
                    y: event.row,
                    selected: 0,
                    actions,
                    view: app.catalog.view,
                    row: app.selection(),
                    filter: app.active_filter().to_owned(),
                    revision: if app.catalog.view == View::Queue {
                        app.queue.revision
                    } else {
                        app.catalog.rows_revision
                    },
                });
            } else {
                app.status = "Selected. Right-click for actions; Enter plays/opens.".into();
            }
        }
        MouseTarget::PlayPause if !right => {
            app.catalog.editing = false;
            app.catalog.filtering = false;
            app.ui.queue.editing = false;
            app.control(Control::Media(MediaAction::Toggle), tx);
        }
        MouseTarget::Seek if !right && app.loaded => {
            if let Some(track) = app.current_track().filter(|t| t.duration_ms > 0) {
                let fraction = event.column.saturating_sub(area.x) as u64;
                let position = (fraction * track.duration_ms.saturating_sub(1) as u64
                    / area.width.saturating_sub(1).max(1) as u64)
                    as u32;
                app.control(Control::Seek(Seek::Position(position)), tx);
            }
        }
        _ => return false,
    }
    true
}
