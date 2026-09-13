use super::*;

pub(super) fn navigation(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let items = View::PRIMARY_TABS
        .iter()
        .enumerate()
        .map(|(i, view)| {
            let active = if *view == app.catalog.view {
                "•"
            } else {
                " "
            };
            let label = match view {
                View::Search => format!("{active} {}  Search", i + 1),
                View::Playlists => format!("{active} {}  Playlists", i + 1),
                View::Liked => format!("{active} {}  Liked Songs", i + 1),
                View::Queue => {
                    let queue_len = app.queue.ids.len();
                    if queue_len > 0 {
                        format!("{active} {}  Queue  · {queue_len}", i + 1)
                    } else {
                        format!("{active} {}  Queue", i + 1)
                    }
                }
                View::Help => format!("{active} {}  Help", i + 1),
                _ => format!("{active} {}  {}", i + 1, view.name()),
            };
            ListItem::new(label).style(Style::default().fg(if *view == app.catalog.view {
                palette.primary_soft
            } else {
                palette.text_muted
            }))
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(Some(if app.catalog.sidebar {
        app.catalog.nav
    } else {
        app.catalog.view.index()
    }));
    let (highlight_style, highlight_symbol) = if app.catalog.sidebar {
        (
            Style::default()
                .fg(palette.text)
                .bg(palette.surface_selected)
                .bold(),
            "▌ ",
        )
    } else {
        (Style::default().fg(palette.primary_soft).bold(), "  ")
    };
    frame.render_stateful_widget(
        List::new(items)
            .block(block_themed(" YOUR LIBRARY ", app.catalog.sidebar, theme))
            .style(Style::default().bg(palette.surface))
            .highlight_style(highlight_style)
            .highlight_symbol(highlight_symbol),
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
            render,
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

pub(super) fn body(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    if app.ui.overlay == Overlay::MixBuilder {
        center(frame, app, render, area);
        return;
    }
    if area.width >= 78 {
        let widths = if area.width >= 116 && app.catalog.view != View::Queue {
            vec![
                Constraint::Length(24),
                Constraint::Min(30),
                Constraint::Length(30),
            ]
        } else {
            vec![Constraint::Length(24), Constraint::Min(20)]
        };
        let body = Layout::horizontal(widths).split(area);
        navigation(frame, app, render, body[0]);
        center(frame, app, render, body[1]);
        if body.len() == 3 {
            queue(frame, app, render, body[2], false);
        }
    } else {
        let body = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
        let nav = View::PRIMARY_TABS
            .iter()
            .enumerate()
            .map(|(i, view)| {
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
                let focused = app.catalog.sidebar && app.catalog.nav == i;
                let active = app.catalog.view == *view;
                Span::styled(
                    format!("{}{}:{} ", if active { "•" } else { "" }, i + 1, label),
                    Style::default()
                        .fg(if focused {
                            palette.primary
                        } else if active {
                            palette.primary_soft
                        } else {
                            palette.text_muted
                        })
                        .bold(),
                )
            })
            .collect::<Vec<_>>();
        let mut x = body[0].x;
        for (index, span) in nav.iter().enumerate() {
            let width = (span.width() as u16).min(body[0].right().saturating_sub(x));
            hit(
                render,
                Rect::new(x, body[0].y, width, body[0].height),
                MouseTarget::Navigation(View::PRIMARY_TABS[index]),
            );
            x += width;
        }
        frame.render_widget(Paragraph::new(Line::from(nav)), body[0]);
        center(frame, app, render, body[1]);
    }
}
