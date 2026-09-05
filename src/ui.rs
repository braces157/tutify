use crate::{
    app::{App, State, View},
    catalog::Rows,
    model::Repeat,
};
use anyhow::Result;
use crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    prelude::*,
    widgets::{
        Block, BorderType, Borders, Cell, Gauge, List, ListItem, ListState, Paragraph, Row, Table,
        TableState, Wrap,
    },
};
use std::io::{Stdout, stdout};

const GREEN: Color = Color::Rgb(30, 215, 96);
const BG: Color = Color::Rgb(14, 17, 16);
const MUTED: Color = Color::Rgb(143, 155, 147);
const FG: Color = Color::Rgb(227, 234, 229);
const BORDER_INACTIVE: Color = Color::Rgb(40, 52, 45);
const HIGHLIGHT_BG: Color = Color::Rgb(24, 45, 32);
const ACCENT_DIM: Color = Color::Rgb(85, 160, 110);

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
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { GREEN } else { BORDER_INACTIVE }))
        .title_style(
            Style::default()
                .fg(if focused { GREEN } else { MUTED })
                .bold(),
        )
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

    let header_line = if area.width >= 65 {
        Line::from(vec![
            Span::styled(
                " TUITIFY ",
                Style::default().fg(Color::Rgb(14, 17, 16)).bg(GREEN).bold(),
            ),
            Span::styled("  YOUR MUSIC, IN THE TERMINAL", Style::default().fg(MUTED)),
            Span::styled(
                "   [? help]  [q quit]",
                Style::default().fg(Color::Rgb(80, 110, 95)),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(" TUITIFY", Style::default().fg(GREEN).bold()),
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
                                GREEN
                            } else {
                                MUTED
                            },
                        )
                        .bold(),
                )
            })
            .collect::<Vec<_>>();
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
            Span::styled(" * ", Style::default().fg(GREEN)),
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
                    .bg(ACCENT_DIM)
                    .bold(),
            ),
            Span::styled(" Play/Pause  ", Style::default().fg(MUTED)),
            Span::styled(
                " / ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(ACCENT_DIM)
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
                    .bg(ACCENT_DIM)
                    .bold(),
            ),
            Span::styled(" Views  ", Style::default().fg(MUTED)),
            Span::styled(
                " Tab ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(ACCENT_DIM)
                    .bold(),
            ),
            Span::styled(" Focus  ", Style::default().fg(MUTED)),
            Span::styled(
                " n/p ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(ACCENT_DIM)
                    .bold(),
            ),
            Span::styled(" Next/Prev  ", Style::default().fg(MUTED)),
            Span::styled(
                " +/- ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(ACCENT_DIM)
                    .bold(),
            ),
            Span::styled(" Vol  ", Style::default().fg(MUTED)),
            Span::styled(
                " ? ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(ACCENT_DIM)
                    .bold(),
            ),
            Span::styled(" Help  ", Style::default().fg(MUTED)),
            Span::styled(
                " q ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(ACCENT_DIM)
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
                    .bg(ACCENT_DIM)
                    .bold(),
            ),
            Span::styled(" Play  ", Style::default().fg(MUTED)),
            Span::styled(
                " / ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(ACCENT_DIM)
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
                    .bg(ACCENT_DIM)
                    .bold(),
            ),
            Span::styled(" Focus  ", Style::default().fg(MUTED)),
            Span::styled(
                " q ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(ACCENT_DIM)
                    .bold(),
            ),
            Span::styled(" Quit", Style::default().fg(MUTED)),
        ])
    };
    frame.render_widget(Paragraph::new(shortcuts), status_split[1]);
}

fn navigation(frame: &mut Frame<'_>, app: &App, area: Rect) {
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
        (Style::default().fg(GREEN).bg(HIGHLIGHT_BG).bold(), "► ")
    } else {
        (Style::default().fg(GREEN).bold(), "  ")
    };
    frame.render_stateful_widget(
        List::new(items)
            .block(block(" LIBRARY ", app.sidebar))
            .highlight_style(hl_style)
            .highlight_symbol(hl_sym),
        area,
        &mut state,
    );
}

fn center(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.view == View::Help {
        let text = "MAKE IT YOURS\n\n\
        NAVIGATION\n\
        1-5 / Tab      Switch views / toggle sidebar focus\n\
        Up/Down, j/k   Move cursor in current view\n\
        Enter          Play track or open playlist\n\
        Backspace      Return from playlist to playlists index\n\
        Esc            Close Help / return to Search\n\n\
        PLAYBACK CONTROLS\n\
        Space          Play, pause, or retry failed playback\n\
        n / p          Next / previous track (restarts after 3s)\n\
        Left / Right   Seek backward / forward 10 seconds\n\
        Home / End     Jump to beginning / end of track\n\n\
        VOLUME CONTROLS\n\
        + / -          Volume up / down 5%\n\
        [ / ]          Fine volume control 1%\n\
        m              Mute / restore previous volume\n\n\
        PLAYBACK MODES & QUEUE\n\
        s              Toggle shuffle (preserves current track)\n\
        r              Cycle repeat: Off -> Queue -> Track\n\
        a              Append selected track to Queue\n\
        Delete         Remove selected item from Queue\n\n\
        CATALOG & NETWORK\n\
        / or f         Filter current view (Liked/Playlists) or Search catalog\n\
        PgDn           Load next catalog page\n\
        F5             Refresh / retry network connection\n\
        q / Ctrl-C     Quit and save state\n\n\
        TROUBLESHOOTING\n\
        Login issue? Exit and run `tuitify auth --force`.\n\
        Streaming issue? Run `tuitify auth --streaming --force`.\n\
        No audio? Check Windows default output device and Spotify Premium.";
        frame.render_widget(
            Paragraph::new(text)
                .block(block(" HELP & SHORTCUTS ", !app.sidebar))
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
        let prompt_line = if app.query.is_empty() && !app.editing {
            Line::from(vec![
                Span::styled(" 🔍 ", Style::default().fg(GREEN)),
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
                Span::styled(" ❯ ", Style::default().fg(GREEN).bold()),
                Span::styled(visible, Style::default().fg(FG).bold()),
            ])
        };
        frame.render_widget(
            Paragraph::new(prompt_line).block(block(
                if app.editing {
                    " 🔍 SEARCH • Enter submit • Esc cancel "
                } else {
                    " 🔍 SEARCH "
                },
                app.editing,
            )),
            split[0],
        );
        split[1]
    } else if (app.view == View::Liked || app.view == View::Playlists)
        && (app.filtering || !app.filter.is_empty())
    {
        let split = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
        let prompt_line = if app.filter.is_empty() && !app.filtering {
            Line::from(vec![
                Span::styled(" 🔍 ", Style::default().fg(GREEN)),
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
                Span::styled(" ❯ ", Style::default().fg(GREEN).bold()),
                Span::styled(visible, Style::default().fg(FG).bold()),
                if app.filtering {
                    Span::styled("▎", Style::default().fg(GREEN))
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
            Paragraph::new(prompt_line).block(block(filter_title, app.filtering)),
            split[0],
        );
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
                        .block(block(title, !app.sidebar))
                        .style(Style::default().fg(MUTED))
                        .wrap(Wrap { trim: false }),
                    body,
                );
            } else {
                let rows: Vec<Row> = indices
                    .iter()
                    .enumerate()
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
                                    .fg(if current { GREEN } else { MUTED })
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
                                    GREEN
                                } else if t.playable {
                                    FG
                                } else {
                                    MUTED
                                })
                                .bold(),
                        );
                        let artist_cell = Cell::from(t.artists.as_str()).style(
                            Style::default().fg(if t.playable { ACCENT_DIM } else { MUTED }),
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

                let mut state = TableState::default().with_selected(Some(app.selected));
                frame.render_stateful_widget(
                    Table::new(rows, widths)
                        .header(header)
                        .block(block(title, !app.sidebar))
                        .row_highlight_style(Style::default().fg(GREEN).bg(HIGHLIGHT_BG).bold()),
                    body,
                    &mut state,
                );
            }
        }
        Rows::Playlists(playlists) => {
            let indices = app.filtered_indices();
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
                        .block(block(title, !app.sidebar))
                        .style(Style::default().fg(MUTED))
                        .wrap(Wrap { trim: false }),
                    body,
                );
            } else {
                let rows: Vec<Row> = indices
                    .iter()
                    .enumerate()
                    .filter_map(|(display_idx, &p_idx)| {
                        let p = playlists.get(p_idx)?;
                        let index_cell = Cell::from(format!("  {:>3}", display_idx + 1))
                            .style(Style::default().fg(MUTED));
                        let name_cell =
                            Cell::from(p.name.as_str()).style(Style::default().fg(FG).bold());
                        let owner_cell =
                            Cell::from(p.owner.as_str()).style(Style::default().fg(ACCENT_DIM));
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
                let mut state = TableState::default().with_selected(Some(app.selected));
                frame.render_stateful_widget(
                    Table::new(rows, widths)
                        .header(header)
                        .block(block(title, !app.sidebar))
                        .row_highlight_style(Style::default().fg(GREEN).bg(HIGHLIGHT_BG).bold()),
                    body,
                    &mut state,
                );
            }
        }
    }
}

fn queue(frame: &mut Frame<'_>, app: &App, area: Rect, main: bool) {
    if app.queue.order.is_empty() {
        let msg = if main {
            "\n  Your queue is empty.\n\n  • Play a track or playlist from Search or Playlists\n  • Press a on any song to append it to the queue"
        } else {
            "\n  Queue is empty.\n  Press a to enqueue."
        };
        frame.render_widget(
            Paragraph::new(msg)
                .block(block(" QUEUE ", main && !app.sidebar))
                .style(Style::default().fg(MUTED)),
            area,
        );
        return;
    }

    let auth_expired = app.status.contains("login expired")
        || app.status.contains("auth --force")
        || app.status.contains("Catalog login failed");

    let rows: Vec<Row> = app
        .queue
        .order
        .iter()
        .enumerate()
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
                    .fg(if is_current { GREEN } else { MUTED })
                    .bold(),
            );
            let name_cell = Cell::from(name).style(if is_current {
                Style::default().fg(GREEN).bold()
            } else if is_placeholder {
                Style::default().fg(MUTED).italic()
            } else {
                Style::default().fg(FG)
            });

            if main {
                let artist_cell = Cell::from(artist)
                    .style(Style::default().fg(if is_placeholder { MUTED } else { ACCENT_DIM }));
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
        Some(app.queue.selected)
    } else {
        app.queue.cursor
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
                .block(block(title, main && !app.sidebar))
                .row_highlight_style(Style::default().fg(GREEN).bg(HIGHLIGHT_BG).bold()),
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
                .block(block(title, main && !app.sidebar))
                .row_highlight_style(Style::default().fg(GREEN).bg(HIGHLIGHT_BG)),
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

    let (badge_text, badge_style) = match app.state {
        State::Paused => (
            " || PAUSED ",
            Style::default().fg(GREEN).bg(Color::Rgb(25, 45, 32)).bold(),
        ),
        State::Playing => (
            " ► PLAYING ",
            Style::default().fg(Color::Rgb(14, 17, 16)).bg(GREEN).bold(),
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

    let row = Layout::horizontal([
        Constraint::Min(12),
        Constraint::Length(if area.width >= 70 { 36 } else { 0 }),
    ])
    .split(parts[0]);

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
        let ctrl_spans = vec![
            Span::styled(vol_str, Style::default().fg(FG).bold()),
            Span::styled("   ", Style::default()),
            Span::styled(
                s_str,
                Style::default().fg(if app.config.shuffle { GREEN } else { MUTED }),
            ),
            Span::styled("   ", Style::default()),
            Span::styled(r_str, Style::default().fg(FG).bold()),
            Span::styled(" ", Style::default()),
        ];
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
            .gauge_style(Style::default().fg(GREEN).bg(Color::Rgb(28, 40, 32))),
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
