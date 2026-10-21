use super::*;

pub(super) fn navigation(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let selected = if app.catalog.sidebar {
        app.catalog.nav
    } else {
        app.catalog.view.index()
    };
    frame.render_widget(
        Block::default().style(Style::default().bg(palette.surface)),
        area,
    );
    let inner = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    let title_row = Rect::new(inner.x, inner.y, inner.width, 1);
    frame.render_widget(
        Paragraph::new("LIBRARY").style(Style::default().fg(palette.text_subtle)),
        title_row,
    );
    let list_area = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
    );
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(
        List::new(View::ALL.iter().map(|view| ListItem::new(view.name())))
            .style(Style::default().fg(palette.text))
            .highlight_style(selected_row_style(theme))
            .highlight_symbol(selected_marker_text(theme)),
        list_area,
        &mut state,
    );
    for (line, view) in View::ALL
        .iter()
        .skip(state.offset())
        .take(list_area.height as usize)
        .enumerate()
    {
        hit(
            render,
            Rect::new(list_area.x, list_area.y + line as u16, list_area.width, 1),
            MouseTarget::Navigation(*view),
        );
    }
}

pub(super) fn top_navigation_layout(area: Rect) -> bool {
    // UI preference: tall windows use tabs/right playback; wide windows retain
    // the sidebar/bottom player. Wallpaper orientation stays physical.
    background::orientation_for_size(area.width, area.height) == background::Orientation::Vertical
}

pub(super) fn docked_player(app: &App, area: Rect) -> bool {
    area.width >= 78
        && area.height >= 24
        && top_navigation_layout(area)
        && app.ui.overlay == Overlay::None
        && app.catalog.view != View::Help
}

/// Whether the window is tall enough to host the docked lyrics column.
pub(super) fn docked_column(area: Rect) -> bool {
    area.width >= 78 && area.height >= 24 && top_navigation_layout(area)
}

pub(super) fn body(
    frame: &mut Frame<'_>,
    app: &App,
    render: &mut RenderState,
    area: Rect,
) -> Option<Rect> {
    if matches!(
        app.ui.overlay,
        Overlay::MixBuilder
            | Overlay::ListeningTools
            | Overlay::LibraryCoverage
            | Overlay::Diagnostics
    ) {
        center(frame, app, render, area);
        return None;
    }
    if !top_navigation_layout(frame.area()) && area.width >= 78 {
        let widths = if area.width >= 116
            && (app.catalog.view != View::Queue
                || (area.height >= 10 && app.ui.overlay == Overlay::None))
        {
            vec![
                Constraint::Length(22),
                Constraint::Min(30),
                Constraint::Length(30),
            ]
        } else {
            vec![Constraint::Length(22), Constraint::Min(20)]
        };
        let columns = Layout::horizontal(widths).split(area);
        sidebar_navigation(frame, app, render, columns[0]);
        center(frame, app, render, columns[1]);
        if columns.len() == 3 {
            if app.catalog.view == View::Queue && app.ui.overlay == Overlay::None {
                spectrum_panel(frame, app, columns[2]);
            } else {
                queue(frame, app, render, columns[2], false);
            }
        }
        return None;
    }
    // Very short terminals keep a scrolling navigation list so every tab can
    // still receive focus when there is only room for one item.
    if area.width >= 78 && area.height < 9 {
        let parts = Layout::horizontal([Constraint::Length(18), Constraint::Min(20)]).split(area);
        navigation(frame, app, render, parts[0]);
        center(frame, app, render, parts[1]);
        return None;
    }
    let parts = Layout::vertical([
        Constraint::Length(if area.width >= 78 { 2 } else { 1 }),
        Constraint::Min(1),
    ])
    .split(area);
    tabs(frame, app, render, parts[0]);
    if docked_player(app, frame.area()) {
        let columns = Layout::horizontal([
            Constraint::Min(if area.width >= 116 { 50 } else { 40 }),
            Constraint::Length(if area.width >= 140 {
                38
            } else if area.width >= 116 {
                34
            } else {
                28
            }),
        ])
        .split(parts[1]);
        center(frame, app, render, columns[0]);
        if app.ui.lyrics_panel {
            // Tall windows with the lyrics column: the player, a compact
            // analyser strip, then the singing lines.
            let rows = Layout::vertical([
                Constraint::Length(10),
                Constraint::Length(2),
                Constraint::Min(4),
            ])
            .split(columns[1]);
            listening_player(frame, app, render, rows[0]);
            spectrum_strip(frame, app, rows[1]);
            lyrics_panel(frame, app, rows[2]);
            return Some(rows[0]);
        }
        let listening =
            Layout::vertical([Constraint::Length(10), Constraint::Min(6)]).split(columns[1]);
        listening_player(frame, app, render, listening[0]);
        if app.catalog.view == View::Queue {
            // In a tall window the analyser takes the whole column: the bars
            // are grounded on a baseline instead of floating in dead space.
            spectrum_panel(frame, app, listening[1]);
        } else {
            queue(frame, app, render, listening[1], false);
        }
        Some(listening[0])
    } else {
        center(frame, app, render, parts[1]);
        None
    }
}

/// The tab strip of tall windows: the active view carries the accent and an
/// underline, the rest stay quiet.
fn tabs(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let tabbed = top_navigation_layout(frame.area());
    frame.render_widget(
        Block::default().style(Style::default().bg(if tabbed {
            palette.surface
        } else {
            palette.background
        })),
        area,
    );
    let nav = View::PRIMARY_TABS
        .iter()
        .enumerate()
        .map(|(index, view)| {
            let label = match view {
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
                    } else if area.width >= 78 {
                        "Liked songs"
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
                _ => view.name(),
            };
            let focused = app.catalog.sidebar && app.catalog.nav == index;
            let active = app.catalog.view == *view;
            let label = if area.width >= 78 {
                format!(" {}  {label}  ", index + 1)
            } else {
                format!("{}{}:{label} ", if active { "•" } else { "" }, index + 1)
            };
            Span::styled(
                label,
                Style::default()
                    .fg(if active {
                        palette.primary
                    } else if focused {
                        palette.primary_soft
                    } else {
                        palette.text_muted
                    })
                    .add_modifier(if active {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            )
        })
        .collect::<Vec<_>>();
    let mut x = area.x;
    for (index, span) in nav.iter().enumerate() {
        let width = (span.width() as u16).min(area.right().saturating_sub(x));
        hit(
            render,
            Rect::new(x, area.y, width, 1),
            MouseTarget::Navigation(View::PRIMARY_TABS[index]),
        );
        if area.height > 1 && app.catalog.view == View::PRIMARY_TABS[index] {
            frame.render_widget(
                Paragraph::new("─".repeat(width as usize))
                    .style(Style::default().fg(palette.primary)),
                Rect::new(x, area.y + 1, width, 1),
            );
        }
        x += width;
    }
    frame.render_widget(
        Paragraph::new(Line::from(nav)),
        Rect::new(area.x, area.y, area.width, 1),
    );
}

/// The navigation column of wide windows: a tone step and a single hairline
/// separate it from the content, instead of a box drawn around it.
fn sidebar_navigation(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    if area.height < 9 {
        navigation(frame, app, render, area);
        return;
    }
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    frame.render_widget(
        Block::default().style(Style::default().bg(palette.surface)),
        area,
    );
    let inner = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    column_rule(
        frame,
        Rect::new(area.right().saturating_sub(1), area.y, 1, area.height),
        theme,
    );
    frame.render_widget(
        Paragraph::new("LIBRARY").style(Style::default().fg(palette.text_subtle)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let roomy = inner.height >= 12;
    let stride = if roomy { 2 } else { 1 };
    let top = inner.y + if roomy { 2 } else { 1 };
    for (index, view) in View::ALL.iter().enumerate() {
        let y = top + index as u16 * stride;
        if y >= inner.bottom() {
            break;
        }
        let active = *view == app.catalog.view;
        let focused = app.catalog.sidebar && app.catalog.nav == index;
        let icon = match view {
            View::Search => "⌕",
            View::Playlists => "≡",
            View::Liked => "♡",
            View::Queue => "▶",
            View::Help => "?",
            _ => "·",
        };
        let label = if *view == View::Liked {
            "Liked songs"
        } else {
            view.name()
        };
        let row = Rect::new(inner.x, y, inner.width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    if focused {
                        "› "
                    } else if active {
                        "▌ "
                    } else {
                        "  "
                    },
                    Style::default().fg(palette.primary),
                ),
                Span::styled(
                    format!("{icon}  {label}"),
                    Style::default()
                        .fg(if active || focused {
                            palette.primary
                        } else {
                            palette.text
                        })
                        .add_modifier(if active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ]))
            .style(Style::default().bg(if active || focused {
                palette.surface_selected
            } else {
                palette.surface_alt
            })),
            row,
        );
        hit(
            render,
            Rect::new(inner.x, y, inner.width, stride.min(inner.bottom() - y)),
            MouseTarget::Navigation(*view),
        );
    }
}
