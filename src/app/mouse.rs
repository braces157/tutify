use super::*;

pub(super) fn format_time(ms: u32) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1_000 % 60)
}

pub(super) fn choose_search(app: &mut App, scope: SearchScope, tasks: &mut Tasks) {
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
        || app.catalog.filter != menu.filter
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
    if app.ui.overlay == Overlay::MixBuilder {
        // Mix Builder is intentionally keyboard-driven in the MVP. Do not let
        // stale underlying hit regions mutate the live queue while it is open.
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
    if app.context_menu.is_some() {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some((_, MouseTarget::Menu(index))) = hit {
                activate_menu(app, index, tasks, tx);
                return true;
            }
        }
        app.context_menu = None;
        return true;
    }
    let Some((area, target)) = hit else {
        return false;
    };
    if event.kind == MouseEventKind::ScrollUp || event.kind == MouseEventKind::ScrollDown {
        if app.ui.overlay == Overlay::Stats {
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
            tasks.view(app, View::Queue);
            app.ui.overlay = Overlay::None;
        }
        app.catalog.editing = false;
        app.catalog.filtering = false;
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
    if app.ui.overlay == Overlay::Stats {
        return false;
    }
    match target {
        MouseTarget::SearchMode(scope) if !right => choose_search(app, scope, tasks),
        MouseTarget::Navigation(view) if !right => {
            app.catalog.history.clear();
            tasks.view(app, view);
            app.ui.overlay = Overlay::None;
        }
        MouseTarget::QueueScroll if right && app.can_undo() => {
            if app.catalog.view != View::Queue {
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
                filter: app.catalog.filter.clone(),
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
        MouseTarget::Catalog(index) | MouseTarget::Queue(index) => {
            if matches!(target, MouseTarget::Queue(_)) {
                if app.catalog.view != View::Queue {
                    tasks.view(app, View::Queue);
                }
                app.ui.overlay = Overlay::None;
                app.queue.selected = index.min(app.queue.order.len().saturating_sub(1));
            } else {
                app.catalog.selected = index;
            }
            app.catalog.editing = false;
            app.catalog.filtering = false;
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
                    filter: app.catalog.filter.clone(),
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
