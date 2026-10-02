use super::*;

pub(super) fn queue(
    frame: &mut Frame<'_>,
    app: &App,
    render: &mut RenderState,
    area: Rect,
    main: bool,
) {
    hit(render, area, MouseTarget::QueueScroll);
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let filtered = main.then(|| app.queue_rows());
    let len = filtered
        .as_ref()
        .map_or(app.queue.order.len(), |rows| rows.len());
    let filtering = main && (app.ui.queue.editing || !app.ui.queue.query.is_empty());
    if !filtering {
        render.queue_filter_metadata_start = None;
    }
    let area = if filtering {
        let split = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
        let parts =
            Layout::horizontal([Constraint::Min(1), Constraint::Length(10)]).split(split[0]);
        let prefix = format!(" {len}/{} / ", app.queue.order.len());
        let mut visible = app.ui.queue.query.clone();
        let available = parts[0].width.saturating_sub(prefix.len() as u16 + 1) as usize;
        while Span::raw(&visible).width() > available {
            visible = visible.chars().skip(1).collect();
        }
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(palette.primary).bold()),
                Span::styled(visible, Style::default().fg(palette.text)),
                Span::styled(
                    if app.ui.queue.editing { "▎" } else { "" },
                    Style::default().fg(palette.primary),
                ),
            ]))
            .style(Style::default().bg(palette.surface_selected)),
            parts[0],
        );
        frame.render_widget(
            Paragraph::new(" Esc Clear").style(
                Style::default()
                    .fg(palette.primary_soft)
                    .bg(palette.surface_selected),
            ),
            parts[1],
        );
        hit(render, parts[0], MouseTarget::QueueFilter);
        hit(render, parts[1], MouseTarget::QueueFilterClear);
        split[1]
    } else {
        area
    };
    let compact = area.height < 4;
    let header = main && area.height >= 6;
    let outer = |title| {
        if compact {
            Block::default().style(Style::default().fg(palette.text).bg(palette.surface))
        } else {
            block_themed(title, main && !app.catalog.sidebar, theme)
        }
    };
    if len == 0 {
        render.queue_height = area.height.saturating_sub(if compact { 0 } else { 2 }) as usize;
        render.queue_scroll = 0;
        let missing = app.ui.queue.missing(&app.queue, &app.cache);
        let message;
        let msg = if !app.queue.order.is_empty() {
            message = if missing > 0 {
                format!(
                    "No matches in loaded info.\n{missing} tracks without info; clear filter to browse. F5 retries metadata."
                )
            } else {
                "No matching tracks.\n/ edit filter | Esc clear".into()
            };
            message.as_str()
        } else if main {
            "\n\n  Your queue is ready.\n\n  Enter  Play a song or playlist\n  a      Add the selected song\n  A      Play selected song next\n  u      Undo the last queue edit"
        } else {
            "\n  Queue is empty\n  Press a to add a track"
        };
        frame.render_widget(
            Paragraph::new(msg)
                .block(outer(" QUEUE ".to_owned()))
                .style(Style::default().fg(palette.text_muted)),
            area,
        );
        return;
    }

    let auth_expired = app.catalog_health == crate::catalog::Health::AuthenticationRequired;

    let selected = if main {
        app.queue_selection().unwrap_or(0)
    } else {
        app.queue.cursor.unwrap_or(0)
    };
    let height = area.height.saturating_sub(if compact {
        0
    } else if header {
        4
    } else {
        2
    }) as usize;
    render.queue_height = height;
    let visible = viewport(selected, len, height, &mut render.queue_scroll);
    if filtering && !visible.is_empty() {
        render.queue_filter_metadata_start = Some(filtered.as_ref().unwrap()[visible.start]);
    }
    for (line, row) in visible.clone().enumerate() {
        let at = filtered.as_ref().map_or(row, |rows| rows[row]);
        hit(
            render,
            Rect::new(
                area.x + u16::from(!compact),
                area.y
                    + if compact {
                        0
                    } else if header {
                        3
                    } else {
                        1
                    }
                    + line as u16,
                area.width.saturating_sub(if compact { 0 } else { 2 }),
                1,
            ),
            MouseTarget::Queue(at),
        );
    }
    let rows: Vec<Row> = visible
        .clone()
        .map(|row| {
            let at = filtered.as_ref().map_or(row, |rows| rows[row]);
            let i = app.queue.order[at];
            let id = &app.queue.ids[i];
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

            let index = if main && area.width < 45 {
                format!("{indicator} {:>2}", at + 1)
            } else {
                format!(" {:>2} {:>3} ", indicator, at + 1)
            };
            let index_cell = Cell::from(index).style(
                Style::default()
                    .fg(if is_current {
                        palette.primary
                    } else {
                        palette.text_subtle
                    })
                    .bold(),
            );
            let name = if app.queue.suggestions.contains(&i) {
                format!("✦ {name}")
            } else {
                name.to_owned()
            };
            let name_cell = Cell::from(name).style(if is_current {
                Style::default().fg(palette.primary).bold()
            } else if is_placeholder {
                Style::default().fg(palette.text_subtle).italic()
            } else {
                Style::default().fg(palette.text)
            });

            if main {
                let artist_cell =
                    Cell::from(artist).style(Style::default().fg(if is_placeholder {
                        palette.text_subtle
                    } else {
                        palette.text_muted
                    }));
                let time_cell =
                    Cell::from(duration).style(Style::default().fg(if is_placeholder {
                        palette.text_subtle
                    } else {
                        palette.text_muted
                    }));
                Row::new(vec![index_cell, name_cell, artist_cell, time_cell])
            } else {
                let time_cell =
                    Cell::from(duration).style(Style::default().fg(if is_placeholder {
                        palette.text_subtle
                    } else {
                        palette.text_muted
                    }));
                Row::new(vec![index_cell, name_cell, time_cell])
            }
        })
        .collect();

    let source = if app.queue.smart_shuffle {
        " | Smart Shuffle • ✦ suggested".into()
    } else if app.radio_epoch == Some(app.queue.epoch) {
        if let Some(error) = &app.radio_error {
            format!(" | Radio unavailable (R retries): {error}")
        } else {
            app.radio_source
                .map(|source| format!(" | {}", source.label()))
                .unwrap_or_else(|| " | Track Radio".into())
        }
    } else {
        String::new()
    };
    let title = if filtering {
        let missing = app.ui.queue.missing(&app.queue, &app.cache);
        format!(
            " QUEUE • {len}/{} matches{}{} ",
            app.queue.ids.len(),
            if missing > 0 {
                format!(" · {missing} without info")
            } else {
                String::new()
            },
            source
        )
    } else if main {
        format!(" QUEUE • {} · / Filter{} ", app.queue.ids.len(), source)
    } else {
        format!(" QUEUE • {}{} ", app.queue.ids.len(), source)
    };
    let mut state = TableState::default().with_selected(if main {
        Some(selected.saturating_sub(visible.start))
    } else {
        app.queue.cursor.map(|c| c.saturating_sub(visible.start))
    });

    if main {
        let widths = if area.width < 45 {
            vec![
                Constraint::Length(5),
                Constraint::Fill(1),
                Constraint::Length(0),
                Constraint::Length(0),
            ]
        } else if area.width >= 60 {
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
        let header_row = Row::new(vec![
            Cell::from("   #    "),
            Cell::from("TITLE"),
            Cell::from("ARTIST"),
            Cell::from(" TIME"),
        ])
        .style(table_header_style(theme))
        .bottom_margin(1);

        let mut table = Table::new(rows, widths)
            .block(outer(title))
            .row_highlight_style(selected_row_style(theme))
            .highlight_symbol(selected_marker(theme));
        if header {
            table = table.header(header_row);
        }
        frame.render_stateful_widget(table, area, &mut state);
    } else {
        let widths = vec![
            Constraint::Length(8),
            Constraint::Fill(1),
            Constraint::Length(6),
        ];
        frame.render_stateful_widget(
            Table::new(rows, widths)
                .block(outer(title))
                .row_highlight_style(selected_row_style(theme))
                .highlight_symbol(selected_marker(theme)),
            area,
            &mut state,
        );
    }
}
