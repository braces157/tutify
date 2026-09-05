use crate::{
    app::{App, State, View},
    catalog::Rows,
};
use anyhow::Result;
use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Gauge, List, ListItem, ListState, Paragraph, Wrap},
};
use std::io::{Stdout, stdout};

const GREEN: Color = Color::Rgb(30, 215, 96);
const BG: Color = Color::Rgb(14, 17, 16);
const MUTED: Color = Color::Rgb(143, 155, 147);
const FG: Color = Color::Rgb(227, 234, 229);

pub struct TerminalGuard {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}
fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(
        stdout(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        crossterm::cursor::Show
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
        if let Err(e) = execute!(stdout(), EnterAlternateScreen, EnableBracketedPaste) {
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

fn block(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(if focused {
            GREEN
        } else {
            Color::Rgb(49, 61, 53)
        }))
}
fn time(ms: u32) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1000 % 60)
}

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().fg(FG).bg(BG)), area);
    if area.width < 32 || area.height < 10 {
        frame.render_widget(
            Paragraph::new("TUITIFY\nResize to 32x10 or larger.\nq quit | Space pause")
                .style(Style::default().fg(GREEN)),
            area,
        );
        return;
    }
    let vertical = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(2),
        Constraint::Length(4),
        Constraint::Length(3),
    ])
    .split(area);
    let header = if area.width >= 65 {
        " TUITIFY  /  YOUR MUSIC, IN THE TERMINAL"
    } else {
        " TUITIFY"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(header, Style::default().fg(GREEN).bold()),
            Span::styled("   ? help", Style::default().fg(MUTED)),
        ])),
        vertical[0],
    );
    if area.width >= 78 {
        let widths = if area.width >= 116 && app.view != View::Queue {
            vec![
                Constraint::Length(19),
                Constraint::Min(30),
                Constraint::Length(30),
            ]
        } else {
            vec![Constraint::Length(19), Constraint::Min(20)]
        };
        let body = Layout::horizontal(widths).split(vertical[1]);
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
                Span::styled(
                    format!(
                        "{}:{} ",
                        i + 1,
                        if area.width < 55 {
                            match v {
                                View::Search => "Find",
                                View::Playlists => "Lists",
                                View::Liked => "Likes",
                                View::Queue => "Q",
                                View::Help => "?",
                            }
                        } else {
                            v.name()
                        }
                    ),
                    Style::default().fg(
                        if (app.sidebar && app.nav == i) || (!app.sidebar && app.view == *v) {
                            GREEN
                        } else {
                            MUTED
                        },
                    ),
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(Line::from(nav)), body[0]);
        center(frame, app, body[1]);
    }
    playback(frame, app, vertical[2]);
    frame.render_widget(
        Paragraph::new(app.status.as_str())
            .style(Style::default().fg(if app.state == State::Failed {
                Color::Yellow
            } else {
                MUTED
            }))
            .wrap(Wrap { trim: true }),
        vertical[3],
    );
}

fn navigation(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let items = View::ALL
        .iter()
        .enumerate()
        .map(|(i, v)| ListItem::new(format!(" {}  {}", i + 1, v.name())))
        .collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(Some(if app.sidebar {
        app.nav
    } else {
        app.view.index()
    }));
    frame.render_stateful_widget(
        List::new(items)
            .block(block(" LIBRARY ", app.sidebar))
            .highlight_style(Style::default().fg(GREEN).bg(Color::Rgb(25, 40, 30)).bold()),
        area,
        &mut state,
    );
}

fn center(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.view == View::Help {
        let text = "MAKE IT YOURS\n\n/          Search songs or paste a Spotify track link\n1-5 / Tab  Switch views / focus navigation\nUp/Down    Move selection (also j/k)\nEnter      Play list / open playlist\nSpace      Pause, resume, or retry failed playback\nn / p      Next / previous (restart after 3 seconds)\nLeft/Right Seek backward / forward 10 seconds\n+ / -      Volume up / down 5%\ns          Shuffle, preserving the current track\nr          Repeat off / queue / track\na          Append selected track without interrupting\nDelete     Remove selected item in Queue\nPgDn       Load the next catalog page\nF5         Refresh / retry network or metadata\nBackspace  Return from playlist to playlist index\nq / Ctrl-C Quit and save; Esc closes Help\n\nEnter builds a queue from the loaded list pages.\nRestart always restores paused. Metadata stays in RAM.\nPlaylist items may require ownership/collaboration.\n\nLogin issue? Exit and run tuitify auth.\nStreaming issue? tuitify auth --streaming.\nNo audio? Check Windows default output and Premium.";
        frame.render_widget(
            Paragraph::new(text)
                .block(block(" HELP ", !app.sidebar))
                .wrap(Wrap { trim: false })
                .scroll((app.selected.min(30) as u16, 0)),
            area,
        );
        return;
    }
    if app.view == View::Queue {
        queue(frame, app, area, true);
        return;
    }
    let body = if app.view == View::Search {
        let split = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
        let prompt = if app.query.is_empty() && !app.editing {
            "Press / to search songs or paste a track link"
        } else {
            app.query.as_str()
        };
        let visible = prompt
            .chars()
            .rev()
            .take(split[0].width.saturating_sub(3) as usize)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        frame.render_widget(
            Paragraph::new(visible).block(block(
                if app.editing {
                    " SEARCH / Enter submit / Esc cancel "
                } else {
                    " SEARCH / "
                },
                app.editing,
            )),
            split[0],
        );
        split[1]
    } else {
        area
    };
    let items: Vec<ListItem> = match &app.rows {
        Rows::Tracks(tracks) => tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let current = app.queue.current() == Some(t.id.as_str());
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} {:>3}  ", if current { ">" } else { " " }, i + 1),
                        Style::default().fg(if current { GREEN } else { MUTED }),
                    ),
                    Span::raw(format!(
                        "{}  /  {}{}",
                        t.name,
                        t.artists,
                        if t.playable { "" } else { " [unavailable]" }
                    )),
                ]))
                .style(Style::default().fg(if t.playable { FG } else { MUTED }))
            })
            .collect(),
        Rows::Playlists(playlists) => playlists
            .iter()
            .enumerate()
            .map(|(i, p)| ListItem::new(format!(" {:>3}  {}  /  {}", i + 1, p.name, p.owner)))
            .collect(),
    };
    let title = format!(
        " {}{} ",
        app.title,
        if app.busy { " / loading" } else { "" }
    );
    if items.is_empty() {
        frame.render_widget(
            Paragraph::new(if app.busy {
                "\n  Fetching from Spotify..."
            } else {
                "\n  No items loaded.\n  / search   2 playlists   3 liked songs"
            })
            .block(block(title, !app.sidebar))
            .wrap(Wrap { trim: false }),
            body,
        );
    } else {
        let mut state = ListState::default().with_selected(Some(app.selected));
        frame.render_stateful_widget(
            List::new(items)
                .block(block(title, !app.sidebar))
                .highlight_style(Style::default().fg(GREEN).bg(Color::Rgb(25, 40, 30)))
                .highlight_symbol(" >"),
            body,
            &mut state,
        );
    }
}

fn queue(frame: &mut Frame<'_>, app: &App, area: Rect, main: bool) {
    let items = app
        .queue
        .order
        .iter()
        .enumerate()
        .map(|(at, i)| {
            let id = &app.queue.ids[*i];
            let name = app.cache.get(id).map(|t| t.name.as_str()).unwrap_or(id);
            ListItem::new(format!(
                "{} {}",
                if app.queue.cursor == Some(at) {
                    ">"
                } else {
                    " "
                },
                name
            ))
            .style(Style::default().fg(if app.queue.cursor == Some(at) {
                GREEN
            } else {
                FG
            }))
        })
        .collect::<Vec<_>>();
    if items.is_empty() {
        frame.render_widget(
            Paragraph::new("\n  Your queue is empty.\n  Play a list or press a\n  to add a track.")
                .block(block(" QUEUE ", main && !app.sidebar)),
            area,
        );
    } else {
        let mut state = ListState::default().with_selected(if main {
            Some(app.queue.selected)
        } else {
            app.queue.cursor
        });
        frame.render_stateful_widget(
            List::new(items)
                .block(block(
                    format!(" QUEUE / {} ", app.queue.ids.len()),
                    main && !app.sidebar,
                ))
                .highlight_style(Style::default().bg(Color::Rgb(25, 40, 30)))
                .highlight_symbol(" "),
            area,
            &mut state,
        );
    }
}

fn playback(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let outer = block(" NOW PLAYING ", false);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let parts = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);
    let track = app.current_track();
    let name = track
        .as_ref()
        .map(|t| t.name.as_str())
        .unwrap_or("Choose a track to begin");
    let state = match app.state {
        State::Paused => "PAUSED",
        State::Playing => "PLAY",
        State::Loading => "LOAD",
        State::Failed => "ERROR",
    };
    let row = Layout::horizontal([
        Constraint::Min(12),
        Constraint::Length(if area.width >= 70 { 34 } else { 0 }),
    ])
    .split(parts[0]);
    frame.render_widget(
        Paragraph::new(format!(" {state}  {name}")).style(Style::default().fg(GREEN)),
        row[0],
    );
    if area.width >= 70 {
        frame.render_widget(
            Paragraph::new(format!(
                "VOL {}%  S:{}  R:{:?}",
                app.config.volume,
                if app.config.shuffle { "ON" } else { "OFF" },
                app.config.repeat
            ))
            .style(Style::default().fg(GREEN))
            .alignment(Alignment::Right),
            row[1],
        );
    }
    let duration = track.as_ref().map(|t| t.duration_ms).unwrap_or(0);
    let ratio = if duration == 0 {
        0.0
    } else {
        (app.queue.position_ms as f64 / duration as f64).clamp(0.0, 1.0)
    };
    let label = format!(
        "{} / {}{}",
        time(app.queue.position_ms),
        time(duration),
        if area.width < 70 {
            format!("  vol {}%", app.config.volume)
        } else {
            String::new()
        }
    );
    frame.render_widget(
        Gauge::default().ratio(ratio).label(label).gauge_style(
            Style::default()
                .fg(Color::Rgb(27, 91, 50))
                .bg(Color::Rgb(25, 30, 27)),
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
