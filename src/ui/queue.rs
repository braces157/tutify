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
    if app.queue.order.is_empty() {
        let msg = if main {
            "\n\n  Your queue is ready.\n\n  Enter  Play a song or playlist\n  a      Add the selected song\n  A      Play selected song next\n  u      Undo the last queue edit"
        } else {
            "\n  Queue is empty\n  Press a to add a track"
        };
        frame.render_widget(
            Paragraph::new(msg)
                .block(block_themed(" QUEUE ", main && !app.catalog.sidebar, theme))
                .style(Style::default().fg(palette.text_muted)),
            area,
        );
        return;
    }

    let auth_expired = app.catalog_health == crate::catalog::Health::AuthenticationRequired;

    let selected = if main {
        app.queue.selected
    } else {
        app.queue.cursor.unwrap_or(0)
    };
    let height = area.height.saturating_sub(if main { 4 } else { 2 }) as usize;
    render.queue_height = height;
    let visible = viewport(
        selected,
        app.queue.order.len(),
        height,
        &mut render.queue_scroll,
    );
    row_hits(render, area, &visible, true, main);
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
                    .fg(if is_current {
                        palette.primary
                    } else {
                        palette.text_subtle
                    })
                    .bold(),
            );
            let name = if app.queue.suggestions.contains(i) {
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
                    Cell::from(duration).style(Style::default().fg(palette.text_subtle));
                Row::new(vec![index_cell, name_cell, artist_cell, time_cell])
            } else {
                let time_cell =
                    Cell::from(duration).style(Style::default().fg(palette.text_subtle));
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
    let title = format!(" QUEUE • {}{} ", app.queue.ids.len(), source);
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
        .style(table_header_style(theme))
        .bottom_margin(1);

        frame.render_stateful_widget(
            Table::new(rows, widths)
                .header(header)
                .block(block_themed(title, main && !app.catalog.sidebar, theme))
                .row_highlight_style(selected_row_style(theme))
                .highlight_symbol(selected_marker(theme)),
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
                .block(block_themed(title, main && !app.catalog.sidebar, theme))
                .row_highlight_style(selected_row_style(theme))
                .highlight_symbol(selected_marker(theme)),
            area,
            &mut state,
        );
    }
}
