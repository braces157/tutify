use super::*;

pub(super) fn navigation(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
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
    let mut state = ListState::default().with_selected(Some(if app.catalog.sidebar {
        app.catalog.nav
    } else {
        app.catalog.view.index()
    }));
    let (hl_style, hl_sym) = if app.catalog.sidebar {
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
            .block(block_themed(" LIBRARY ", app.catalog.sidebar, theme))
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
        let body = Layout::horizontal(widths).spacing(1).split(area);
        navigation(frame, app, render, body[0]);
        center(frame, app, render, body[1]);
        if body.len() == 3 {
            queue(frame, app, render, body[2], false);
        }
    } else {
        let body = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
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
                            if (app.catalog.sidebar && app.catalog.nav == i)
                                || (!app.catalog.sidebar && app.catalog.view == *v)
                            {
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
                render,
                Rect::new(x, body[0].y, width, body[0].height),
                MouseTarget::Navigation(View::ALL[index]),
            );
            x += width;
        }
        frame.render_widget(Paragraph::new(Line::from(nav)), body[0]);
        center(frame, app, render, body[1]);
    }
}
