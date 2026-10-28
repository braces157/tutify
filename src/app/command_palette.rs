use super::listening::ListeningTool;
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UiCommand {
    Shuffle,
    Repeat,
    Theme,
    Lyrics,
    LyricsPanel,
    Visualizer,
    Stats,
    MixBuilder,
    ListeningTools,
    RetryMetadata,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaletteCommand {
    Action(Action),
    Control(Control),
    Listening(ListeningTool),
    Ui(UiCommand),
    View(View),
}

#[derive(Clone, Copy)]
pub(crate) struct CommandSpec {
    command: PaletteCommand,
    pub label: &'static str,
    aliases: &'static str,
    pub shortcut: &'static str,
}

#[derive(Default)]
pub struct CommandPalette {
    pub query: String,
    pub selected: usize,
}

macro_rules! command {
    ($command:expr, $label:literal, $aliases:literal, $shortcut:literal) => {
        CommandSpec {
            command: $command,
            label: $label,
            aliases: $aliases,
            shortcut: $shortcut,
        }
    };
}

const COMMANDS: &[CommandSpec] = &[
    command!(
        PaletteCommand::Action(Action::PlaySelected),
        "Play selected track or open playlist",
        "listen start",
        "Enter"
    ),
    command!(
        PaletteCommand::Action(Action::EnqueueSelected),
        "Add selected track or playlist to queue",
        "enqueue append",
        "e"
    ),
    command!(
        PaletteCommand::Action(Action::PlayNext),
        "Play selected track next",
        "priority queue",
        "Ctrl+Enter"
    ),
    command!(
        PaletteCommand::Action(Action::StartRadio),
        "Start radio from selected track",
        "discover related recommendations",
        "R"
    ),
    command!(
        PaletteCommand::Action(Action::ClearQueue),
        "Clear entire queue",
        "empty remove all",
        "C"
    ),
    command!(
        PaletteCommand::Action(Action::MoveUp),
        "Move selected queue entry up",
        "reorder",
        "K"
    ),
    command!(
        PaletteCommand::Action(Action::MoveDown),
        "Move selected queue entry down",
        "reorder",
        "J"
    ),
    command!(
        PaletteCommand::Action(Action::RemoveSelected),
        "Remove selected queue entry",
        "delete",
        "d / x / Delete"
    ),
    command!(
        PaletteCommand::Action(Action::Undo),
        "Undo queue edit",
        "restore",
        "u / Ctrl+Z"
    ),
    command!(
        PaletteCommand::Action(Action::ViewAlbum),
        "View selected track album",
        "browse record",
        "a"
    ),
    command!(
        PaletteCommand::Action(Action::ViewArtist),
        "View selected track artist",
        "browse musician",
        "A"
    ),
    command!(
        PaletteCommand::Control(Control::Media(MediaAction::Toggle)),
        "Play or pause playback",
        "resume retry stop",
        "Space"
    ),
    command!(
        PaletteCommand::Control(Control::Media(MediaAction::Next)),
        "Next track",
        "skip forward",
        "n"
    ),
    command!(
        PaletteCommand::Control(Control::Media(MediaAction::Previous)),
        "Previous track",
        "back",
        "p (outside Album/Artist)"
    ),
    command!(
        PaletteCommand::Control(Control::Seek(Seek::Relative(-10000))),
        "Seek backward 10 seconds",
        "rewind",
        "Left"
    ),
    command!(
        PaletteCommand::Control(Control::Seek(Seek::Relative(10000))),
        "Seek forward 10 seconds",
        "fast forward",
        "Right"
    ),
    command!(
        PaletteCommand::Control(Control::Seek(Seek::Start)),
        "Seek to track beginning",
        "restart",
        "Home"
    ),
    command!(
        PaletteCommand::Control(Control::Seek(Seek::End)),
        "Seek to track end",
        "finish",
        "End"
    ),
    command!(
        PaletteCommand::Control(Control::Volume(5)),
        "Increase volume",
        "volume up louder sound",
        "+"
    ),
    command!(
        PaletteCommand::Control(Control::Volume(-5)),
        "Decrease volume",
        "volume down quieter sound",
        "-"
    ),
    command!(
        PaletteCommand::Control(Control::Mute),
        "Mute or restore volume",
        "silent sound",
        "m"
    ),
    command!(
        PaletteCommand::Ui(UiCommand::Shuffle),
        "Cycle shuffle mode",
        "random smart suggestions",
        "s"
    ),
    command!(
        PaletteCommand::Ui(UiCommand::Repeat),
        "Cycle repeat mode",
        "loop song queue",
        "r (outside Lyrics)"
    ),
    command!(
        PaletteCommand::Ui(UiCommand::Theme),
        "Cycle theme",
        "colors appearance",
        "t"
    ),
    command!(
        PaletteCommand::Ui(UiCommand::Lyrics),
        "Toggle lyrics view",
        "words karaoke",
        "l"
    ),
    command!(
        PaletteCommand::Ui(UiCommand::LyricsPanel),
        "Toggle docked lyrics panel",
        "words tall portrait",
        "L"
    ),
    command!(
        PaletteCommand::Ui(UiCommand::Visualizer),
        "Toggle visualizer",
        "spectrum fft",
        "v"
    ),
    command!(
        PaletteCommand::Ui(UiCommand::Stats),
        "Open song statistics",
        "plays history listening time",
        "S"
    ),
    command!(
        PaletteCommand::Ui(UiCommand::MixBuilder),
        "Open Mix Builder",
        "playlist recipe",
        "M"
    ),
    command!(
        PaletteCommand::Ui(UiCommand::ListeningTools),
        "Toggle listening tools",
        "sleep timer cleanup connect",
        "F6"
    ),
    command!(
        PaletteCommand::Ui(UiCommand::RetryMetadata),
        "Refresh data and retry metadata",
        "reload recheck access",
        "F5"
    ),
    command!(
        PaletteCommand::View(View::Search),
        "Search music",
        "browse navigation",
        "1"
    ),
    command!(
        PaletteCommand::View(View::Playlists),
        "View playlists",
        "browse navigation",
        "2"
    ),
    command!(
        PaletteCommand::View(View::Liked),
        "View Liked Songs",
        "browse navigation",
        "3"
    ),
    command!(
        PaletteCommand::View(View::Queue),
        "View queue",
        "browse navigation",
        "4"
    ),
    command!(
        PaletteCommand::View(View::Help),
        "Show help and shortcuts",
        "browse navigation",
        "? / F1 / 5"
    ),
];

pub(crate) fn matches(app: &App) -> Vec<CommandSpec> {
    let Some(palette) = &app.ui.palette else {
        return Vec::new();
    };
    let query = palette.query.to_lowercase();
    let tokens: Vec<_> = query.split_whitespace().collect();
    let mut rows: Vec<_> = COMMANDS
        .iter()
        .copied()
        .chain(ListeningTool::ALL.into_iter().map(|tool| CommandSpec {
            command: PaletteCommand::Listening(tool),
            label: tool.label(),
            aliases: tool.aliases(),
            shortcut: "F6 > Enter",
        }))
        .filter_map(|spec| {
            let label = spec.label.to_lowercase();
            let searchable = format!("{label} {} {}", spec.aliases, spec.shortcut.to_lowercase());
            tokens
                .iter()
                .all(|token| searchable.contains(token))
                .then(|| {
                    let score = if tokens.iter().all(|token| label.contains(token)) {
                        0
                    } else {
                        1
                    };
                    (score, spec)
                })
        })
        .collect();
    rows.sort_by_key(|(score, _)| *score);
    rows.into_iter().map(|(_, spec)| spec).collect()
}

impl CommandSpec {
    pub(crate) fn unavailable(self, app: &App) -> Option<&'static str> {
        match self.command {
            PaletteCommand::Action(action) => {
                if action != Action::Undo {
                    if app.ui.overlay != Overlay::None {
                        return Some("Close the overlay to select a track.");
                    }
                    if app.catalog.sidebar {
                        return Some("Focus the track list first with Tab.");
                    }
                }
                actions::unavailable(app, action)
            }
            PaletteCommand::Listening(tool) => tool.unavailable(app),
            PaletteCommand::Control(Control::Media(_))
                if app.queue.ids.is_empty() && !app.loaded =>
            {
                Some("Add tracks to the queue first.")
            }
            PaletteCommand::Control(control) => control.unavailable(app),
            PaletteCommand::View(view) => jobs::view_unavailable(app, view),
            _ => None,
        }
    }
}

fn reset_selection(app: &mut App) {
    let selected = if app
        .ui
        .palette
        .as_ref()
        .is_some_and(|palette| palette.query.trim().is_empty())
    {
        matches(app)
            .iter()
            .position(|row| row.unavailable(app).is_none())
            .unwrap_or(0)
    } else {
        0
    };
    if let Some(palette) = &mut app.ui.palette {
        palette.selected = selected;
    }
    let render = app.ui.render.get_mut();
    render.palette_scroll = 0;
    render.mouse_hits.clear();
}

pub(super) fn open(app: &mut App) {
    app.ui.palette = Some(CommandPalette::default());
    reset_selection(app);
}

pub(super) fn paste(app: &mut App, text: &str) {
    if let Some(palette) = &mut app.ui.palette {
        let remaining = 100usize.saturating_sub(palette.query.chars().count());
        palette
            .query
            .extend(text.chars().filter(|c| !c.is_control()).take(remaining));
    }
    reset_selection(app);
}

fn close(app: &mut App) {
    app.ui.palette = None;
    app.ui.render.get_mut().mouse_hits.clear();
}

fn execute(app: &mut App, tasks: &mut Tasks, tx: &mpsc::UnboundedSender<Command>) {
    let Some(palette) = &app.ui.palette else {
        return;
    };
    let Some(spec) = matches(app).get(palette.selected).copied() else {
        return;
    };
    if let Some(reason) = spec.unavailable(app) {
        app.status = reason.into();
        return;
    }
    close(app);
    app.context_menu = None;
    match spec.command {
        PaletteCommand::Action(action) => actions::apply(app, action, tasks, tx),
        PaletteCommand::Control(control) => app.control(control, tx),
        PaletteCommand::Listening(tool) => app.apply_listening_tool(tool, tasks, tx),
        PaletteCommand::Ui(command) => input::apply_ui_command(app, command, tasks, tx),
        PaletteCommand::View(view) => {
            app.catalog.history.clear();
            tasks.view(app, view);
        }
    }
}

pub(super) fn key(
    app: &mut App,
    event: KeyEvent,
    tasks: &mut Tasks,
    tx: &mpsc::UnboundedSender<Command>,
) {
    let len = matches(app).len();
    let last = len.saturating_sub(1);
    let palette = app.ui.palette.as_mut().unwrap();
    match event.code {
        KeyCode::Esc => close(app),
        KeyCode::Enter => execute(app, tasks, tx),
        KeyCode::Up => palette.selected = palette.selected.saturating_sub(1),
        KeyCode::Down => palette.selected = (palette.selected + 1).min(last),
        KeyCode::PageUp => palette.selected = palette.selected.saturating_sub(8),
        KeyCode::PageDown => palette.selected = (palette.selected + 8).min(last),
        KeyCode::Home => palette.selected = 0,
        KeyCode::End => palette.selected = last,
        KeyCode::Backspace => {
            palette.query.pop();
            reset_selection(app);
        }
        KeyCode::Char(c)
            if !c.is_control()
                && !event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            paste(app, &c.to_string())
        }
        _ => (),
    }
}

pub(super) fn mouse(
    app: &mut App,
    event: MouseEvent,
    tasks: &mut Tasks,
    tx: &mpsc::UnboundedSender<Command>,
) {
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
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => key(
            app,
            KeyEvent::new(
                if event.kind == MouseEventKind::ScrollUp {
                    KeyCode::Up
                } else {
                    KeyCode::Down
                },
                KeyModifiers::NONE,
            ),
            tasks,
            tx,
        ),
        MouseEventKind::Down(MouseButton::Left) => match target {
            Some(MouseTarget::PaletteRow(at)) if at < matches(app).len() => {
                app.ui.palette.as_mut().unwrap().selected = at;
            }
            Some(MouseTarget::PaletteApply) => execute(app, tasks, tx),
            Some(MouseTarget::PaletteClose) => close(app),
            _ => (),
        },
        _ => (),
    }
}
