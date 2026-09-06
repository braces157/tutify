use crate::{
    app::{App, MouseTarget, State, View},
    catalog::Rows,
    model::Repeat,
};
use anyhow::Result;
use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, SetTitle, disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{
    prelude::*,
    widgets::{
        Block, BorderType, Borders, Cell, Clear, Gauge, List, ListItem, ListState, Paragraph, Row,
        Table, TableState, Wrap,
    },
};
use std::io::{Stdout, stdout};

const BG: Color = Color::Rgb(14, 17, 16);
const MUTED: Color = Color::Rgb(143, 155, 147);
const FG: Color = Color::Rgb(227, 234, 229);

pub struct TerminalGuard {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}
fn restore() {
    // On Windows mouse capture restores the console mode saved while raw mode
    // was active. Release it first so it cannot re-enable raw input after exit.
    let _ = execute!(stdout(), DisableMouseCapture);
    let _ = disable_raw_mode();
    let _ = execute!(
        stdout(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        crossterm::cursor::Show,
        crossterm::style::Print("\x1b[23;0t")
    );
}
impl TerminalGuard {
    pub fn enter() -> Result<Self> {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
        enable_raw_mode()?;
        if let Err(e) = execute!(
            stdout(),
            crossterm::style::Print("\x1b[22;0t"),
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableMouseCapture,
            SetTitle("Tuitify")
        ) {
            restore();
            return Err(e.into());
        }
        match Terminal::new(CrosstermBackend::new(stdout())) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(e) => {
                restore();
                Err(e.into())
            }
        }
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

pub fn set_title(title: &str) {
    let _ = execute!(stdout(), SetTitle(title));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Spotify,
    Amber,
    Matrix,
    Cyberpunk,
    Monochrome,
}

impl Theme {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "amber" => Self::Amber,
            "matrix" => Self::Matrix,
            "cyberpunk" => Self::Cyberpunk,
            "monochrome" => Self::Monochrome,
            _ => Self::Spotify,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify Green",
            Self::Amber => "Amber CRT",
            Self::Matrix => "Matrix Green",
            Self::Cyberpunk => "Cyberpunk Cyan",
            Self::Monochrome => "Monochrome",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spotify => "spotify",
            Self::Amber => "amber",
            Self::Matrix => "matrix",
            Self::Cyberpunk => "cyberpunk",
            Self::Monochrome => "monochrome",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Spotify => Self::Amber,
            Self::Amber => Self::Matrix,
            Self::Matrix => Self::Cyberpunk,
            Self::Cyberpunk => Self::Monochrome,
            Self::Monochrome => Self::Spotify,
        }
    }
    pub fn primary(self) -> Color {
        match self {
            Self::Spotify => Color::Rgb(30, 215, 96),
            Self::Amber => Color::Rgb(255, 176, 0),
            Self::Matrix => Color::Rgb(0, 255, 102),
            Self::Cyberpunk => Color::Rgb(0, 229, 255),
            Self::Monochrome => Color::Rgb(240, 240, 240),
        }
    }
    pub fn highlight_bg(self) -> Color {
        match self {
            Self::Spotify => Color::Rgb(24, 45, 32),
            Self::Amber => Color::Rgb(50, 35, 10),
            Self::Matrix => Color::Rgb(10, 40, 20),
            Self::Cyberpunk => Color::Rgb(30, 20, 50),
            Self::Monochrome => Color::Rgb(40, 40, 40),
        }
    }
    pub fn border_inactive(self) -> Color {
        match self {
            Self::Spotify => Color::Rgb(40, 52, 45),
            Self::Amber => Color::Rgb(60, 45, 25),
            Self::Matrix => Color::Rgb(20, 50, 30),
            Self::Cyberpunk => Color::Rgb(40, 35, 65),
            Self::Monochrome => Color::Rgb(55, 55, 55),
        }
    }
    pub fn accent_dim(self) -> Color {
        match self {
            Self::Spotify => Color::Rgb(85, 160, 110),
            Self::Amber => Color::Rgb(180, 120, 20),
            Self::Matrix => Color::Rgb(40, 170, 75),
            Self::Cyberpunk => Color::Rgb(255, 0, 127),
            Self::Monochrome => Color::Rgb(160, 160, 160),
        }
    }
}

pub fn generate_bars(frame: u32, count: usize, is_playing: bool) -> String {
    if !is_playing {
        return " ".repeat(count);
    }
    const BLOCKS: [char; 8] = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let mut s = String::with_capacity(count * 4);
    let t = frame as f64 * 0.16;
    for i in 0..count {
        let x = i as f64;
        let freq = 1.15 + (x * 0.18);
        let wave = (t * freq + x * 0.9).sin() * 0.42
            + (t * 0.65 - x * 0.45).cos() * 0.36
            + (t * 2.3 + x * 1.4).sin() * 0.22;
        let norm = ((wave + 1.0) * 0.5).clamp(0.0, 0.999);
        let idx = (norm * 8.0) as usize;
        s.push(BLOCKS[idx.min(7)]);
    }
    s
}

fn block_themed(title: impl Into<Line<'static>>, focused: bool, theme: Theme) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused {
            theme.primary()
        } else {
            theme.border_inactive()
        }))
        .title_style(
            Style::default()
                .fg(if focused { theme.primary() } else { MUTED })
                .bold(),
        )
}
fn time(ms: u32) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1000 % 60)
}

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    app.mouse_hits.borrow_mut().clear();
    app.terminal_size.set((area.width, area.height));
    let theme = Theme::from_str(&app.config.theme);
    frame.render_widget(Block::default().style(Style::default().fg(FG).bg(BG)), area);
    if area.width < 32 || area.height < 10 {
        frame.render_widget(
            Paragraph::new("TUITIFY\nResize to 32x10 or larger.\nq quit | Space pause")
                .style(Style::default().fg(theme.primary())),
            area,
        );
        return;
    }
    let compact = area.height < 18;
    let vertical = Layout::vertical([
        Constraint::Length(if compact { 1 } else { 2 }),
        Constraint::Min(3),
        Constraint::Length(4),
        Constraint::Length(if compact { 1 } else { 3 }),
    ])
    .split(area);

    let theme = Theme::from_str(&app.config.theme);
    let header_line = if area.width >= 65 {
        Line::from(vec![
            Span::styled(
                " TUITIFY ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.primary())
                    .bold(),
            ),
            Span::styled(
                format!(" [{}]", theme.name()),
                Style::default().fg(theme.primary()).bold(),
            ),
            Span::styled("  YOUR MUSIC, IN THE TERMINAL", Style::default().fg(MUTED)),
            Span::styled(
                "   [? help]  [q quit]  [t theme]",
                Style::default().fg(theme.accent_dim()),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(" TUITIFY", Style::default().fg(theme.primary()).bold()),
            Span::styled("  ? help", Style::default().fg(MUTED)),
        ])
    };
    frame.render_widget(Paragraph::new(header_line), vertical[0]);

    if area.width >= 78 {
        let widths = if area.width >= 116 && app.view != View::Queue {
            vec![
                Constraint::Length(24),
                Constraint::Min(30),
                Constraint::Length(30),
            ]
        } else {
            vec![Constraint::Length(24), Constraint::Min(20)]
        };
        let body = Layout::horizontal(widths).spacing(1).split(vertical[1]);
        navigation(frame, app, body[0]);
        center(frame, app, body[1]);
        if body.len() == 3 {
            queue(frame, app, body[2], false);
        }
    } else {
        let body = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(vertical[1]);
        let nav = View::ALL
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let label = match v {
                    View::Search => {
                        if area.width < 55 {
                            "Find"
                        } else {
                            "Search"
                        }
                    }
                    View::Playlists => {
                        if area.width < 55 {
                            "Lists"
                        } else {
                            "Playlists"
                        }
                    }
                    View::Liked => {
                        if area.width < 55 {
                            "Likes"
                        } else {
                            "Liked"
                        }
                    }
                    View::Queue => {
                        if area.width < 55 {
                            "Q"
                        } else {
                            "Queue"
                        }
                    }
                    View::Help => {
                        if area.width < 55 {
                            "?"
                        } else {
                            "Help"
                        }
                    }
                };
                Span::styled(
                    format!("{}:{} ", i + 1, label),
                    Style::default()
                        .fg(
                            if (app.sidebar && app.nav == i) || (!app.sidebar && app.view == *v) {
                                theme.primary()
                            } else {
                                MUTED
                            },
                        )
                        .bold(),
                )
            })
            .collect::<Vec<_>>();
        let mut x = body[0].x;
        for (index, span) in nav.iter().enumerate() {
            let width = (span.width() as u16).min(body[0].right().saturating_sub(x));
            hit(
                app,
                Rect::new(x, body[0].y, width, body[0].height),
                MouseTarget::Navigation(View::ALL[index]),
            );
            x += width;
        }
        frame.render_widget(Paragraph::new(Line::from(nav)), body[0]);
        center(frame, app, body[1]);
    }
    playback(frame, app, vertical[2]);

    let status_split =
        Layout::vertical([Constraint::Length(1), Constraint::Length(2)]).split(vertical[3]);

    let auth_expired = app.status.contains("login expired")
        || app.status.contains("auth --force")
        || app.status.contains("Catalog login failed");

    let status_line = if auth_expired {
        Line::from(vec![
            Span::styled(
                " ! AUTH EXPIRED ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(Color::Yellow)
                    .bold(),
            ),
            Span::styled(
                "  Spotify login expired. Exit (q) and run 'tuitify auth --force' in terminal to reconnect.",
                Style::default().fg(Color::Yellow).bold(),
            ),
        ])
    } else if app.state == State::Failed {
        Line::from(vec![
            Span::styled(" ! ", Style::default().fg(Color::LightRed).bold()),
            Span::styled(&app.status, Style::default().fg(Color::LightRed)),
        ])
    } else if app.busy {
        Line::from(vec![
            Span::styled(" ... ", Style::default().fg(Color::Yellow).bold()),
            Span::styled(&app.status, Style::default().fg(Color::Yellow)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" * ", Style::default().fg(theme.primary())),
            Span::styled(&app.status, Style::default().fg(MUTED)),
        ])
    };
    frame.render_widget(
        Paragraph::new(status_line).wrap(Wrap { trim: true }),
        status_split[0],
    );

    let shortcuts = if area.width >= 80 {
        Line::from(vec![
            Span::styled(
                " Space ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Play/Pause  ", Style::default().fg(MUTED)),
            Span::styled(
                " / ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(
                if matches!(app.view, View::Liked | View::Playlists) {
                    " Filter  "
                } else {
                    " Search  "
                },
                Style::default().fg(MUTED),
            ),
            Span::styled(
                " 1-5 ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Views  ", Style::default().fg(MUTED)),
            Span::styled(
                " Tab ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Focus  ", Style::default().fg(MUTED)),
            Span::styled(
                " n/p ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Next/Prev  ", Style::default().fg(MUTED)),
            Span::styled(
                " +/- ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Vol  ", Style::default().fg(MUTED)),
            Span::styled(
                " ? ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Help  ", Style::default().fg(MUTED)),
            Span::styled(
                " q ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Quit", Style::default().fg(MUTED)),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                " Space ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Play  ", Style::default().fg(MUTED)),
            Span::styled(
                " / ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(
                if matches!(app.view, View::Liked | View::Playlists) {
                    " Filter  "
                } else {
                    " Search  "
                },
                Style::default().fg(MUTED),
            ),
            Span::styled(
                " Tab ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Focus  ", Style::default().fg(MUTED)),
            Span::styled(
                " q ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Quit", Style::default().fg(MUTED)),
        ])
    };
    frame.render_widget(Paragraph::new(shortcuts), status_split[1]);
    if let Some(menu) = &app.context_menu {
        let width = 28.min(area.width);
        let height = (menu.actions.len() as u16 + 2).min(area.height);
        let rect = Rect::new(
            menu.x.min(area.right().saturating_sub(width)).max(area.x),
            menu.y.min(area.bottom().saturating_sub(height)).max(area.y),
            width,
            height,
        );
        frame.render_widget(Clear, rect);
        let items = menu.actions.iter().map(|(label, _)| ListItem::new(*label));
        let mut state = ListState::default().with_selected(Some(menu.selected));
        frame.render_stateful_widget(
            List::new(items)
                .block(block_themed(" Actions • Esc close ", true, theme))
                .style(Style::default().fg(FG).bg(BG))
                .highlight_style(
                    Style::default()
                        .fg(theme.primary())
                        .bg(theme.highlight_bg())
                        .bold(),
                ),
            rect,
            &mut state,
        );
        // While a menu is open, clicks cannot activate the covered controls.
        app.mouse_hits.borrow_mut().clear();
        for index in 0..menu.actions.len().min(height.saturating_sub(2) as usize) {
            hit(
                app,
                Rect::new(
                    rect.x + 1,
                    rect.y + 1 + index as u16,
                    width.saturating_sub(2),
                    1,
                ),
                MouseTarget::Menu(index),
            );
        }
    }
}

fn hit(app: &App, area: Rect, target: MouseTarget) {
    if area.width > 0 && area.height > 0 {
        app.mouse_hits.borrow_mut().push((area, target));
    }
}
fn row_hits(app: &App, area: Rect, visible: &std::ops::Range<usize>, queue: bool, header: bool) {
    let top = area.y + if header { 3 } else { 1 };
    for (line, index) in visible.clone().enumerate() {
        let y = top + line as u16;
        if y >= area.bottom().saturating_sub(1) {
            break;
        }
        hit(
            app,
            Rect::new(area.x + 1, y, area.width.saturating_sub(2), 1),
            if queue {
                MouseTarget::Queue(index)
            } else {
                MouseTarget::Catalog(index)
            },
        );
    }
}

fn navigation(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let items = View::ALL
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let label = match v {
                View::Search => format!(" {} Search", i + 1),
                View::Playlists => format!(" {} Playlists", i + 1),
                View::Liked => format!(" {} Liked Songs", i + 1),
                View::Queue => {
                    let q_len = app.queue.ids.len();
                    if q_len > 0 {
                        format!(" {} Queue ({})", i + 1, q_len)
                    } else {
                        format!(" {} Queue", i + 1)
                    }
                }
                View::Help => format!(" {} Help", i + 1),
            };
            ListItem::new(label)
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(Some(if app.sidebar {
        app.nav
    } else {
        app.view.index()
    }));
    let (hl_style, hl_sym) = if app.sidebar {
        (
            Style::default()
                .fg(theme.primary())
                .bg(theme.highlight_bg())
                .bold(),
            "► ",
        )
    } else {
        (Style::default().fg(theme.primary()).bold(), "  ")
    };
    frame.render_stateful_widget(
        List::new(items)
            .block(block_themed(" LIBRARY ", app.sidebar, theme))
            .highlight_style(hl_style)
            .highlight_symbol(hl_sym),
        area,
        &mut state,
    );
    for (line, view) in View::ALL
        .iter()
        .skip(state.offset())
        .take(area.height.saturating_sub(2) as usize)
        .enumerate()
    {
        hit(
            app,
            Rect::new(
                area.x + 1,
                area.y + 1 + line as u16,
                area.width.saturating_sub(2),
                1,
            ),
            MouseTarget::Navigation(*view),
        );
    }
}

fn visualizer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let outer = block_themed(" RETRO VISUALIZER [v exit] ", true, theme);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let parts = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(4),
        Constraint::Length(2),
    ])
    .split(inner);

    let track = app.current_track();
    let track_info = if let Some(t) = &track {
        format!("{} - {}", t.name, t.artists)
    } else {
        "No track playing".to_string()
    };
    let status_line = Line::from(vec![
        Span::styled(
            if app.state == State::Playing {
                " ► PLAYING: "
            } else {
                " || PAUSED: "
            },
            Style::default().fg(theme.primary()).bold(),
        ),
        Span::styled(track_info, Style::default().fg(FG).bold()),
        Span::styled(
            "   [DECORATIVE ANIMATION]",
            Style::default().fg(theme.accent_dim()).italic(),
        ),
    ]);
    frame.render_widget(Paragraph::new(status_line), parts[0]);

    let height = parts[1].height as usize;
    let width = parts[1].width as usize;
    if height >= 2 && width >= 10 {
        let is_playing = app.state == State::Playing;
        let bar_count = (width / 3).clamp(8, 32);
        let t = app.animation_frame as f64 * 0.16;

        let heights: Vec<usize> = (0..bar_count)
            .map(|col| {
                if !is_playing {
                    return 0;
                }
                let x = col as f64;
                let freq = 1.1 + x * 0.12;
                let wave = (t * freq + x * 0.7).sin() * 0.42
                    + (t * 0.6 - x * 0.35).cos() * 0.35
                    + (t * 2.3 + x * 1.4).sin() * 0.23;
                ((wave + 1.0) * 0.5 * height as f64).round() as usize
            })
            .collect();
        let mut lines = Vec::new();
        for row in (1..=height).rev() {
            let mut spans = vec![Span::raw("  ")];
            for &val in &heights {
                if val >= row {
                    let color = if row > height * 3 / 4 {
                        Color::Red
                    } else if row > height / 2 {
                        Color::Yellow
                    } else {
                        theme.primary()
                    };
                    spans.push(Span::styled("█ ", Style::default().fg(color).bold()));
                } else if val + 1 == row && is_playing {
                    spans.push(Span::styled("▄ ", Style::default().fg(theme.accent_dim())));
                } else {
                    spans.push(Span::styled("  ", Style::default()));
                }
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), parts[1]);
    }

    let labels = Line::from(vec![Span::styled(
        "  Decorative bars • not audio frequency analysis",
        Style::default().fg(MUTED),
    )]);
    frame.render_widget(Paragraph::new(labels), parts[2]);
}

fn lyrics(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let track = app.current_track();
    let title = if let Some(t) = &track {
        format!(" LYRICS • {} [l exit] ", t.name)
    } else {
        " LYRICS [l exit] ".to_string()
    };
    let outer = block_themed(title, true, theme);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    if let Some(error) = &app.lyrics_error {
        frame.render_widget(
            Paragraph::new(error.as_str())
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(MUTED)),
            inner,
        );
        return;
    }
    if app.lyrics_loading {
        let p = Paragraph::new("\n  ⟳ Loading synchronized lyrics from Lrclib...")
            .style(Style::default().fg(theme.primary()).italic());
        frame.render_widget(p, inner);
        return;
    }

    let Some(lyr) = &app.lyrics else {
        let p = Paragraph::new("\n  No lyrics available for this track.\n\n  • Press l to return to library view\n  • Songs with available lyrics will sync automatically")
            .style(Style::default().fg(MUTED));
        frame.render_widget(p, inner);
        return;
    };

    if !lyr.lines.is_empty() {
        let active = lyr.current_line_index(app.queue.position_ms);
        let current_idx = active.unwrap_or(0);
        let height = inner.height as usize;
        let half = height / 2;
        let start = current_idx.saturating_sub(half);
        let visible = lyr.lines.iter().enumerate().skip(start).take(height);

        let items: Vec<Line<'static>> = visible
            .map(|(i, l)| {
                if Some(i) == active {
                    Line::from(vec![
                        Span::styled("► ", Style::default().fg(theme.primary()).bold()),
                        Span::styled(
                            l.text.clone(),
                            Style::default().fg(FG).bg(theme.highlight_bg()).bold(),
                        ),
                    ])
                } else if active.is_some_and(|active| i < active) {
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled(l.text.clone(), Style::default().fg(MUTED)),
                    ])
                } else {
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled(l.text.clone(), Style::default().fg(FG)),
                    ])
                }
            })
            .collect();
        frame.render_widget(Paragraph::new(items), inner);
    } else if let Some(plain) = &lyr.plain {
        let p = Paragraph::new(plain.as_str())
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(FG));
        let max_scroll = p
            .line_count(inner.width)
            .saturating_sub(inner.height as usize);
        app.lyrics_length.set(max_scroll + 1);
        frame.render_widget(
            p.scroll((
                app.lyrics_scroll.min(max_scroll).min(u16::MAX as usize) as u16,
                0,
            )),
            inner,
        );
    } else {
        let p = Paragraph::new("\n  No lyrics found for this track. Press l to return.")
            .style(Style::default().fg(MUTED));
        frame.render_widget(p, inner);
    }
}

fn viewport(
    selected: usize,
    len: usize,
    height: usize,
    offset: &std::cell::Cell<usize>,
) -> std::ops::Range<usize> {
    let height = height.max(1);
    let selected = selected.min(len.saturating_sub(1));
    let mut start = offset.get().min(len.saturating_sub(height));
    if selected < start {
        start = selected;
    }
    if selected >= start + height {
        start = selected + 1 - height;
    }
    offset.set(start);
    start..(start + height).min(len)
}

fn center(frame: &mut Frame<'_>, app: &App, area: Rect) {
    hit(app, area, MouseTarget::CatalogScroll);
    let theme = Theme::from_str(&app.config.theme);
    if app.show_visualizer {
        visualizer(frame, app, area);
        return;
    }
    if app.show_lyrics {
        lyrics(frame, app, area);
        return;
    }
    if app.view == View::Help {
        let text = "MAKE IT YOURS\n\n\
        NAVIGATION\n\
        1-5 / Tab      Switch views / toggle sidebar focus\n\
        Up/Down, j/k   Move cursor in current view\n\
        Enter          Play track or open playlist\n\
        Backspace      Return from playlist to playlists index\n\
        Esc            Close Help / exit Lyrics or Visualizer\n\n\
        MOUSE CONTROLS\n\
        Left click     Select row / switch view / edit search\n\
        Right click    Track or playlist actions (Esc closes)\n\
        Wheel          Move three rows; scroll Help/plain lyrics\n\
        Badge / bar    Play-pause / seek\n\
        Ctrl+Shift+V   Intentional terminal paste into search/filter\n\n\
        PLAYBACK CONTROLS\n\
        Space          Play, pause, or retry failed playback\n\
        n / p          Next / previous track (restarts after 3s)\n\
        Left / Right   Seek backward / forward 10 seconds\n\
        Home / End     Jump to beginning / end of track\n\n\
        VOLUME CONTROLS\n\
        + / -          Volume up / down 5%\n\
        [ / ]          Fine volume control 1%\n\
        m              Mute / restore previous volume\n\n\
        QUEUE & PLAYLISTS\n\
        a              Append selected track to Queue (or enqueue entire playlist)\n\
        A (Shift-A)    Play Next (insert directly after current track)\n\
        R (Shift-R)    Start Track Radio (play track & queue related recommendations)\n\
        K / J          Move selected track Up / Down in Queue\n\
        d / x / Delete Remove selected item from Queue\n\
        C (Shift-C)    Clear entire Queue\n\
        . or c         Jump to currently playing track in Queue\n\
        s              Toggle shuffle (preserves current track)\n\
        r              Cycle repeat: Off -> Queue -> Track\n\n\
        RETRO FEATURES & THEMES\n\
        t              Cycle Retro Theme (Spotify, Amber CRT, Matrix, Cyberpunk, Monochrome)\n\
        v              Toggle Decorative Retro Visualizer\n\
        l              Toggle Synced Real-Time Lyrics View (Lrclib)\n\n\
        CATALOG & NETWORK\n\
        / or f         Filter current view (Liked/Playlists) or Search catalog\n\
        PgDn           Load next catalog page\n\
        F5             Refresh / retry network connection\n\
        q / Ctrl-C     Quit and save state\n\n\
        TROUBLESHOOTING\n\
        Login issue? Exit and run `tuitify auth --force`.\n\
        Streaming issue? Run `tuitify auth --streaming --force`.\n\
        No audio? Check Windows default output device and Spotify Premium.";
        let outer = block_themed(" HELP & SHORTCUTS ", !app.sidebar, theme);
        let inner = outer.inner(area);
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
        let max_scroll = paragraph
            .line_count(inner.width)
            .saturating_sub(inner.height as usize);
        app.help_length.set(max_scroll + 1);
        frame.render_widget(outer, area);
        frame.render_widget(
            paragraph.scroll((
                app.selected.min(max_scroll).min(u16::MAX as usize) as u16,
                0,
            )),
            inner,
        );
        return;
    }
    if app.view == View::Queue {
        queue(frame, app, area, true);
        return;
    }
    let body = if app.view == View::Search {
        let split = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
        let prompt_line = if app.query.is_empty() && !app.editing {
            Line::from(vec![
                Span::styled(" 🔍 ", Style::default().fg(theme.primary())),
                Span::styled(
                    "Press / to search songs, artists, or paste a Spotify link",
                    Style::default().fg(MUTED).italic(),
                ),
            ])
        } else {
            let visible = app
                .query
                .chars()
                .rev()
                .take(split[0].width.saturating_sub(6) as usize)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>();
            Line::from(vec![
                Span::styled(" ❯ ", Style::default().fg(theme.primary()).bold()),
                Span::styled(visible, Style::default().fg(FG).bold()),
            ])
        };
        frame.render_widget(
            Paragraph::new(prompt_line).block(block_themed(
                if app.editing {
                    " 🔍 SEARCH • Enter submit • Esc cancel "
                } else {
                    " 🔍 SEARCH "
                },
                app.editing,
                theme,
            )),
            split[0],
        );
        hit(app, split[0], MouseTarget::Prompt);
        split[1]
    } else if (app.view == View::Liked || app.view == View::Playlists)
        && (app.filtering || !app.filter.is_empty())
    {
        let split = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
        let prompt_line = if app.filter.is_empty() && !app.filtering {
            Line::from(vec![
                Span::styled(" 🔍 ", Style::default().fg(theme.primary())),
                Span::styled(
                    "Press / or f to filter",
                    Style::default().fg(MUTED).italic(),
                ),
            ])
        } else {
            let visible = app
                .filter
                .chars()
                .rev()
                .take(split[0].width.saturating_sub(8) as usize)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>();
            Line::from(vec![
                Span::styled(" ❯ ", Style::default().fg(theme.primary()).bold()),
                Span::styled(visible, Style::default().fg(FG).bold()),
                if app.filtering {
                    Span::styled("▎", Style::default().fg(theme.primary()))
                } else {
                    Span::raw("")
                },
            ])
        };
        let filter_title = if app.filtering {
            " 🔍 FILTER • Enter play • Down browse • Esc clear "
        } else {
            " 🔍 FILTER • [/] edit • Esc clear "
        };
        frame.render_widget(
            Paragraph::new(prompt_line).block(block_themed(filter_title, app.filtering, theme)),
            split[0],
        );
        hit(app, split[0], MouseTarget::Prompt);
        split[1]
    } else {
        area
    };

    let title = if app.is_filtered() {
        let matched = app.filtered_indices().len();
        let total = app.raw_len();
        format!(
            " {} • {} of {} matches{} ",
            app.title,
            matched,
            total,
            if app.busy { " • Loading..." } else { "" }
        )
    } else {
        format!(
            " {}{} ",
            app.title,
            if app.busy { " • Loading..." } else { "" }
        )
    };

    match &app.rows {
        Rows::Tracks(tracks) => {
            let indices = app.filtered_indices();
            let visible = viewport(
                app.selected,
                indices.len(),
                body.height.saturating_sub(4) as usize,
                &app.catalog_scroll,
            );
            row_hits(app, body, &visible, false, true);
            if tracks.is_empty() || (app.is_filtered() && indices.is_empty()) {
                let empty_msg = if app.busy {
                    "\n  ⟳ Fetching tracks from Spotify..."
                } else if app.is_filtered() {
                    "\n  No tracks match your filter.\n\n  • Backspace to edit filter\n  • Esc to clear filter and show all tracks"
                } else {
                    "\n  No tracks found.\n\n  • Press / to search for songs or paste a track link\n  • Press 2 to browse your playlists\n  • Press 3 to see your liked songs"
                };
                frame.render_widget(
                    Paragraph::new(empty_msg)
                        .block(block_themed(title, !app.sidebar, theme))
                        .style(Style::default().fg(MUTED))
                        .wrap(Wrap { trim: false }),
                    body,
                );
            } else {
                let rows: Vec<Row> = indices
                    .iter()
                    .enumerate()
                    .skip(visible.start)
                    .take(visible.len())
                    .filter_map(|(display_idx, &track_idx)| {
                        let t = tracks.get(track_idx)?;
                        let current = app.queue.current() == Some(t.id.as_str());
                        let indicator = if current {
                            if app.state == State::Playing {
                                "►"
                            } else {
                                "||"
                            }
                        } else {
                            "  "
                        };
                        let index_cell =
                            Cell::from(format!(" {:>2} {:>3} ", indicator, display_idx + 1)).style(
                                Style::default()
                                    .fg(if current { theme.primary() } else { MUTED })
                                    .bold(),
                            );
                        let title_cell = Cell::from(format!(
                            "{}{}",
                            t.name,
                            if t.playable { "" } else { " [unavailable]" }
                        ))
                        .style(
                            Style::default()
                                .fg(if current {
                                    theme.primary()
                                } else if t.playable {
                                    FG
                                } else {
                                    MUTED
                                })
                                .bold(),
                        );
                        let artist_cell = Cell::from(t.artists.as_str()).style(
                            Style::default().fg(if t.playable {
                                theme.accent_dim()
                            } else {
                                MUTED
                            }),
                        );
                        let time_cell = Cell::from(if t.duration_ms > 0 {
                            time(t.duration_ms)
                        } else {
                            "--:--".to_string()
                        })
                        .style(Style::default().fg(MUTED));

                        Some(Row::new(vec![
                            index_cell,
                            title_cell,
                            artist_cell,
                            time_cell,
                        ]))
                    })
                    .collect();

                let widths = if body.width >= 60 {
                    vec![
                        Constraint::Length(8),
                        Constraint::Percentage(50),
                        Constraint::Percentage(34),
                        Constraint::Length(7),
                    ]
                } else {
                    vec![
                        Constraint::Length(8),
                        Constraint::Percentage(60),
                        Constraint::Percentage(34),
                        Constraint::Length(0),
                    ]
                };
                let header = Row::new(vec![
                    Cell::from("   #    "),
                    Cell::from("TITLE"),
                    Cell::from("ARTIST"),
                    Cell::from(" TIME"),
                ])
                .style(Style::default().fg(MUTED).bold())
                .bottom_margin(1);

                let mut state = TableState::default()
                    .with_selected(Some(app.selected.saturating_sub(visible.start)));
                frame.render_stateful_widget(
                    Table::new(rows, widths)
                        .header(header)
                        .block(block_themed(title, !app.sidebar, theme))
                        .row_highlight_style(
                            Style::default()
                                .fg(theme.primary())
                                .bg(theme.highlight_bg())
                                .bold(),
                        ),
                    body,
                    &mut state,
                );
            }
        }
        Rows::Playlists(playlists) => {
            let indices = app.filtered_indices();
            let visible = viewport(
                app.selected,
                indices.len(),
                body.height.saturating_sub(4) as usize,
                &app.catalog_scroll,
            );
            row_hits(app, body, &visible, false, true);
            if playlists.is_empty() || (app.is_filtered() && indices.is_empty()) {
                let empty_msg = if app.busy {
                    "\n  ⟳ Fetching playlists from Spotify..."
                } else if app.is_filtered() {
                    "\n  No playlists match your filter.\n\n  • Backspace to edit filter\n  • Esc to clear filter and show all playlists"
                } else {
                    "\n  No playlists found."
                };
                frame.render_widget(
                    Paragraph::new(empty_msg)
                        .block(block_themed(title, !app.sidebar, theme))
                        .style(Style::default().fg(MUTED))
                        .wrap(Wrap { trim: false }),
                    body,
                );
            } else {
                let rows: Vec<Row> = indices
                    .iter()
                    .enumerate()
                    .skip(visible.start)
                    .take(visible.len())
                    .filter_map(|(display_idx, &p_idx)| {
                        let p = playlists.get(p_idx)?;
                        let index_cell = Cell::from(format!("  {:>3}", display_idx + 1))
                            .style(Style::default().fg(MUTED));
                        let name_cell =
                            Cell::from(p.name.as_str()).style(Style::default().fg(FG).bold());
                        let owner_cell = Cell::from(p.owner.as_str())
                            .style(Style::default().fg(theme.accent_dim()));
                        Some(Row::new(vec![index_cell, name_cell, owner_cell]))
                    })
                    .collect();
                let widths = vec![
                    Constraint::Length(6),
                    Constraint::Percentage(60),
                    Constraint::Percentage(34),
                ];
                let header = Row::new(vec![
                    Cell::from("  #"),
                    Cell::from("PLAYLIST"),
                    Cell::from("OWNER"),
                ])
                .style(Style::default().fg(MUTED).bold())
                .bottom_margin(1);
                let mut state = TableState::default()
                    .with_selected(Some(app.selected.saturating_sub(visible.start)));
                frame.render_stateful_widget(
                    Table::new(rows, widths)
                        .header(header)
                        .block(block_themed(title, !app.sidebar, theme))
                        .row_highlight_style(
                            Style::default()
                                .fg(theme.primary())
                                .bg(theme.highlight_bg())
                                .bold(),
                        ),
                    body,
                    &mut state,
                );
            }
        }
    }
}

fn queue(frame: &mut Frame<'_>, app: &App, area: Rect, main: bool) {
    hit(app, area, MouseTarget::QueueScroll);
    let theme = Theme::from_str(&app.config.theme);
    if app.queue.order.is_empty() {
        let msg = if main {
            "\n  Your queue is empty.\n\n  • Play a track or playlist from Search or Playlists\n  • Press a on any song to append it to the queue"
        } else {
            "\n  Queue is empty.\n  Press a to enqueue."
        };
        frame.render_widget(
            Paragraph::new(msg)
                .block(block_themed(" QUEUE ", main && !app.sidebar, theme))
                .style(Style::default().fg(MUTED)),
            area,
        );
        return;
    }

    let auth_expired = app.status.contains("login expired")
        || app.status.contains("auth --force")
        || app.status.contains("Catalog login failed");

    let selected = if main {
        app.queue.selected
    } else {
        app.queue.cursor.unwrap_or(0)
    };
    let height = area.height.saturating_sub(if main { 4 } else { 2 }) as usize;
    app.queue_height.set(height);
    let visible = viewport(selected, app.queue.order.len(), height, &app.queue_scroll);
    row_hits(app, area, &visible, true, main);
    let rows: Vec<Row> = app
        .queue
        .order
        .iter()
        .enumerate()
        .skip(visible.start)
        .take(visible.len())
        .map(|(at, i)| {
            let id = &app.queue.ids[*i];
            let is_current = app.queue.cursor == Some(at);
            let indicator = if is_current {
                if app.state == State::Playing {
                    "►"
                } else {
                    "||"
                }
            } else {
                "  "
            };

            let (name, artist, duration, is_placeholder) = if let Some(t) = app.cache.get(id) {
                (
                    t.name.as_str(),
                    t.artists.as_str(),
                    if t.duration_ms > 0 {
                        time(t.duration_ms)
                    } else {
                        "--:--".to_string()
                    },
                    false,
                )
            } else if app.metadata_error.is_some() {
                (
                    "Track info unavailable; F5 retry",
                    "—",
                    "--:--".to_string(),
                    true,
                )
            } else if auth_expired {
                (
                    "Track info unavailable (auth expired)",
                    "—",
                    "--:--".to_string(),
                    true,
                )
            } else {
                ("Loading track info...", "—", "--:--".to_string(), true)
            };

            let index_cell = Cell::from(format!(" {:>2} {:>3} ", indicator, at + 1)).style(
                Style::default()
                    .fg(if is_current { theme.primary() } else { MUTED })
                    .bold(),
            );
            let name_cell = Cell::from(name).style(if is_current {
                Style::default().fg(theme.primary()).bold()
            } else if is_placeholder {
                Style::default().fg(MUTED).italic()
            } else {
                Style::default().fg(FG)
            });

            if main {
                let artist_cell =
                    Cell::from(artist).style(Style::default().fg(if is_placeholder {
                        MUTED
                    } else {
                        theme.accent_dim()
                    }));
                let time_cell = Cell::from(duration).style(Style::default().fg(MUTED));
                Row::new(vec![index_cell, name_cell, artist_cell, time_cell])
            } else {
                let time_cell = Cell::from(duration).style(Style::default().fg(MUTED));
                Row::new(vec![index_cell, name_cell, time_cell])
            }
        })
        .collect();

    let title = format!(" QUEUE • {} ", app.queue.ids.len());
    let mut state = TableState::default().with_selected(if main {
        Some(app.queue.selected.saturating_sub(visible.start))
    } else {
        app.queue.cursor.map(|c| c.saturating_sub(visible.start))
    });

    if main {
        let widths = if area.width >= 60 {
            vec![
                Constraint::Length(8),
                Constraint::Percentage(50),
                Constraint::Percentage(34),
                Constraint::Length(7),
            ]
        } else {
            vec![
                Constraint::Length(8),
                Constraint::Percentage(60),
                Constraint::Percentage(34),
                Constraint::Length(0),
            ]
        };
        let header = Row::new(vec![
            Cell::from("   #    "),
            Cell::from("TITLE"),
            Cell::from("ARTIST"),
            Cell::from(" TIME"),
        ])
        .style(Style::default().fg(MUTED).bold())
        .bottom_margin(1);

        frame.render_stateful_widget(
            Table::new(rows, widths)
                .header(header)
                .block(block_themed(title, main && !app.sidebar, theme))
                .row_highlight_style(
                    Style::default()
                        .fg(theme.primary())
                        .bg(theme.highlight_bg())
                        .bold(),
                ),
            area,
            &mut state,
        );
    } else {
        let widths = vec![
            Constraint::Length(8),
            Constraint::Fill(1),
            Constraint::Length(6),
        ];
        frame.render_stateful_widget(
            Table::new(rows, widths)
                .block(block_themed(title, main && !app.sidebar, theme))
                .row_highlight_style(
                    Style::default()
                        .fg(theme.primary())
                        .bg(theme.highlight_bg()),
                ),
            area,
            &mut state,
        );
    }
}

fn playback(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let outer = block_themed(" NOW PLAYING ", false, theme);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let parts = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);
    let track = app.current_track();

    let (badge_text, badge_style) = match app.state {
        State::Paused => (
            " || PAUSED ",
            Style::default()
                .fg(theme.primary())
                .bg(theme.highlight_bg())
                .bold(),
        ),
        State::Playing => (
            " ► PLAYING ",
            Style::default()
                .fg(Color::Rgb(14, 17, 16))
                .bg(theme.primary())
                .bold(),
        ),
        State::Loading => (
            " ... LOAD ",
            Style::default()
                .fg(Color::Yellow)
                .bg(Color::Rgb(40, 35, 20))
                .bold(),
        ),
        State::Failed => (
            " ! ERROR ",
            Style::default()
                .fg(Color::Red)
                .bg(Color::Rgb(45, 20, 20))
                .bold(),
        ),
    };

    let (ctrl_width, bar_count) = if area.width >= 90 {
        (48, 10)
    } else if area.width >= 70 {
        (38, 6)
    } else if area.width >= 50 && app.state == State::Playing {
        (8, 6)
    } else {
        (0, 0)
    };

    let row =
        Layout::horizontal([Constraint::Min(12), Constraint::Length(ctrl_width)]).split(parts[0]);

    let mut track_spans = vec![Span::styled(badge_text, badge_style)];
    if let Some(t) = &track {
        track_spans.push(Span::styled(
            format!("  {}", t.name),
            Style::default().fg(FG).bold(),
        ));
        if !t.artists.is_empty() {
            track_spans.push(Span::styled(
                format!("  •  {}", t.artists),
                Style::default().fg(MUTED),
            ));
        }
    } else {
        track_spans.push(Span::styled(
            "  Choose a track to begin",
            Style::default().fg(MUTED),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(track_spans)), row[0]);
    hit(
        app,
        Rect::new(row[0].x, row[0].y, 10.min(row[0].width), row[0].height),
        MouseTarget::PlayPause,
    );
    hit(app, parts[1], MouseTarget::Seek);

    if ctrl_width > 0 {
        let mut ctrl_spans = Vec::new();
        if app.state == State::Playing && bar_count > 0 {
            let mini_bars = generate_bars(app.animation_frame, bar_count, true);
            ctrl_spans.push(Span::styled(
                mini_bars,
                Style::default().fg(theme.primary()).bold(),
            ));
            if area.width >= 70 {
                ctrl_spans.push(Span::styled("   ", Style::default()));
            }
        }
        if area.width >= 70 {
            let vol_str = if app.config.volume == 0 {
                "VOL MUTED".to_owned()
            } else {
                format!("VOL {}%", app.config.volume)
            };
            let s_str = if app.config.shuffle {
                "SHUF:ON"
            } else {
                "SHUF:OFF"
            };
            let r_str = match app.config.repeat {
                Repeat::Off => "R:Off",
                Repeat::Queue => "R:Queue",
                Repeat::Track => "R:Track",
            };
            ctrl_spans.push(Span::styled(vol_str, Style::default().fg(FG).bold()));
            ctrl_spans.push(Span::styled("   ", Style::default()));
            ctrl_spans.push(Span::styled(
                s_str,
                Style::default().fg(if app.config.shuffle {
                    theme.primary()
                } else {
                    MUTED
                }),
            ));
            ctrl_spans.push(Span::styled("   ", Style::default()));
            ctrl_spans.push(Span::styled(r_str, Style::default().fg(FG).bold()));
            ctrl_spans.push(Span::styled(" ", Style::default()));
        }
        frame.render_widget(
            Paragraph::new(Line::from(ctrl_spans)).alignment(Alignment::Right),
            row[1],
        );
    }

    let duration = track.as_ref().map(|t| t.duration_ms).unwrap_or(0);
    let elapsed = app.queue.position_ms.min(duration);
    let ratio = if duration == 0 {
        0.0
    } else {
        elapsed as f64 / duration as f64
    };
    let label = if duration == 0 {
        format!(
            "{} / --:--{}",
            time(elapsed),
            if area.width < 70 {
                format!("  vol {}%", app.config.volume)
            } else {
                String::new()
            }
        )
    } else {
        let percent = (elapsed as u64 * 100 / duration as u64).min(100);
        format!(
            "{} / {}  {}%  -{}{}",
            time(elapsed),
            time(duration),
            percent,
            time(duration.saturating_sub(elapsed)),
            if area.width < 70 {
                format!("  vol {}%", app.config.volume)
            } else {
                String::new()
            }
        )
    };
    frame.render_widget(
        Gauge::default()
            .ratio(ratio)
            .label(Span::styled(
                label,
                Style::default().fg(Color::White).bold(),
            ))
            .use_unicode(true)
            .gauge_style(
                Style::default()
                    .fg(theme.primary())
                    .bg(theme.highlight_bg()),
            ),
        parts[1],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::Track, queue::Queue, storage::Config};
    use ratatui::backend::TestBackend;
    #[test]
    fn help_and_plain_lyrics_can_scroll_to_last_line_in_small_terminal() {
        let mut app = App::new(Config::default(), Queue::default());
        app.view = View::Help;
        app.selected = usize::MAX;
        let mut terminal = Terminal::new(TestBackend::new(32, 10)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("Premium."), "{text}");
        assert!(app.help_length.get() > 30);
        app.show_lyrics = true;
        app.lyrics = Some(crate::lyrics::Lyrics {
            lines: vec![],
            plain: Some(
                (0..100)
                    .map(|i| format!("Line {i}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        });
        app.lyrics_scroll = usize::MAX;
        terminal.draw(|f| draw(f, &app)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("Line 99"), "{text}");
    }
    #[test]
    fn viewport_tracks_selection_without_building_offscreen_rows() {
        let offset = std::cell::Cell::new(0);
        assert_eq!(viewport(4999, 5000, 12, &offset), 4988..5000);
        assert_eq!(viewport(4998, 5000, 12, &offset), 4988..5000);
        assert_eq!(viewport(0, 5000, 12, &offset), 0..12);
    }
    #[test]
    fn queue_uses_selected_theme_accent() {
        let mut app = App::new(Config::default(), Queue::default());
        app.config.theme = "amber".into();
        app.queue.replace(vec!["0".repeat(22)], 0, false);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        assert!(
            buffer
                .content
                .iter()
                .any(|c| c.fg == Theme::Amber.primary())
        );
        assert!(
            !buffer
                .content
                .iter()
                .any(|c| c.fg == Theme::Spotify.primary())
        );
    }
    #[test]
    #[ignore = "Release microbenchmark; run with --release --ignored --nocapture"]
    fn benchmark_render_scaling() {
        for count in [50, 500, 5000] {
            let mut app = App::new(Config::default(), Queue::default());
            app.queue
                .replace((0..count).map(|i| format!("{i:022}")).collect(), 0, false);
            let tracks: Vec<Track> = (0..count)
                .map(|i| Track {
                    id: format!("{i:022}"),
                    name: format!("Benchmark song {i}"),
                    artists: "Benchmark artist".into(),
                    duration_ms: 200000,
                    playable: true,
                })
                .collect();
            for track in tracks.iter().take(50) {
                app.cache.insert(track.id.clone(), track.clone());
            }
            for mode in ["queue", "filtered"] {
                app.view = if mode == "queue" {
                    View::Queue
                } else {
                    View::Liked
                };
                if mode == "filtered" {
                    app.rows = Rows::Tracks(tracks.clone());
                    app.filter = "benchmark artist".into();
                }
                let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
                terminal.draw(|f| draw(f, &app)).unwrap();
                let start = std::time::Instant::now();
                for _ in 0..100 {
                    terminal.draw(|f| draw(f, &app)).unwrap();
                }
                println!(
                    "mode={mode} rows={count} mean_frame_ms={:.3} release={}",
                    start.elapsed().as_secs_f64() * 10.0,
                    !cfg!(debug_assertions)
                );
            }
        }
    }
    #[test]
    fn render_all_views_normal_narrow_and_tiny() {
        for (w, h) in [(120, 35), (80, 24), (48, 18), (32, 10), (20, 6)] {
            for view in View::ALL {
                let mut app = App::new(Config::default(), Queue::default());
                app.view = view;
                let mut t = Track::unknown(&"0".repeat(22));
                t.name = "A song with Unicode: café 日本語".into();
                app.queue.replace(vec![t.id.clone()], 0, false);
                app.cache.insert(t.id.clone(), t.clone());
                app.rows = Rows::Tracks(vec![t]);
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                terminal.draw(|f| draw(f, &app)).unwrap();
                let text = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect::<String>();
                assert!(text.contains("TUITIFY"));
                if w >= 32 && h >= 10 {
                    assert!(text.contains("PAUSED"));
                }
                if w >= 70 {
                    assert!(text.contains("VOL 50%"));
                    assert!(text.contains("R:Off"));
                }
            }
        }
    }
    #[test]
    fn render_playback_timestamp_and_muted_volume() {
        let mut app = App::new(Config::default(), Queue::default());
        let mut track = Track::unknown(&"0".repeat(22));
        track.name = "Timestamp check".into();
        track.duration_ms = 210_000;
        app.queue.replace(vec![track.id.clone()], 0, false);
        app.queue.position_ms = 23_000;
        app.cache.insert(track.id.clone(), track);
        app.config.volume = 0;
        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("0:23 / 3:30  10%  -3:07"));
        assert!(text.contains("VOL MUTED"));
    }
    #[test]
    fn render_visualizer_bars_and_smooth_animation() {
        let mut app = App::new(Config::default(), Queue::default());
        let mut track = Track::unknown(&"0".repeat(22));
        track.name = "Moon Landing Plan".into();
        track.artists = "tuki.".into();
        track.duration_ms = 242_000;
        app.queue.replace(vec![track.id.clone()], 0, false);
        app.queue.position_ms = 157_000;
        app.cache.insert(track.id.clone(), track);
        app.state = State::Playing;
        app.animation_frame = 42;

        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();

        assert!(text.contains("Moon Landing Plan"));
        assert!(text.contains("tuki."));
        assert!(text.contains("2:37 / 4:02  64%  -1:25"));
        assert!(text.contains("VOL 50%"));
        let has_eq_bar = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█']
            .iter()
            .any(|&c| text.contains(c));
        assert!(has_eq_bar);
    }
    #[test]
    fn render_modern_ui_elements() {
        let mut app = App::new(Config::default(), Queue::default());
        app.view = View::Queue;
        let mut t1 = Track::unknown(&"0".repeat(22));
        t1.name = "My Song".into();
        t1.artists = "My Artist".into();
        t1.duration_ms = 180_000;
        let uncached_id = "1".repeat(22);
        app.queue
            .replace(vec![t1.id.clone(), uncached_id.clone()], 0, false);
        app.cache.insert(t1.id.clone(), t1);
        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("TITLE"));
        assert!(text.contains("ARTIST"));
        assert!(text.contains("TIME"));
        assert!(text.contains("My Song"));
        assert!(text.contains("My Artist"));
        assert!(text.contains("Loading track info..."));
        assert!(!text.contains(&uncached_id));
        assert!(text.contains("QUEUE • 2"));
        assert!(text.contains("Space"));
        assert!(text.contains("Play/Pause"));
    }
    #[test]
    fn render_filter_prompt_and_filtered_tracks() {
        let mut app = App::new(Config::default(), Queue::default());
        app.view = View::Liked;
        app.title = "Liked Songs".into();
        app.rows = Rows::Tracks(vec![
            Track {
                id: "1".into(),
                name: "Bohemian Rhapsody".into(),
                artists: "Queen".into(),
                duration_ms: 354000,
                playable: true,
            },
            Track {
                id: "2".into(),
                name: "Yellow".into(),
                artists: "Coldplay".into(),
                duration_ms: 269000,
                playable: true,
            },
        ]);
        app.filtering = true;
        app.filter = "queen".into();

        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();

        assert!(text.contains("FILTER"));
        assert!(text.contains("queen"));
        assert!(text.contains("1 of 2 matches"));
        assert!(text.contains("Bohemian Rhapsody"));
        assert!(!text.contains("Coldplay"));

        // When no matches
        app.filter = "jazz".into();
        terminal.draw(|f| draw(f, &app)).unwrap();
        let text2 = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text2.contains("No tracks match your filter"));
    }
    #[test]
    #[ignore = "Requires a real terminal; exercises alternate screen and a caught panic"]
    fn terminal_cleanup_acceptance() {
        {
            let _guard = TerminalGuard::enter().unwrap();
            assert!(crossterm::terminal::is_raw_mode_enabled().unwrap());
        }
        assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
        let caught = std::panic::catch_unwind(|| {
            let _guard = TerminalGuard::enter().unwrap();
            panic!("intentional terminal restoration test");
        });
        assert!(caught.is_err());
        assert!(!crossterm::terminal::is_raw_mode_enabled().unwrap());
        println!("PASS: raw mode restored after normal exit and a caught panic");
    }
}
