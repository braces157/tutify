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
    if area.height == 0 || area.width == 0 {
        return;
    }
    let spacious = main && area.height >= 12;
    let title_rows = if main && area.height >= 18 {
        5
    } else if spacious {
        3
    } else {
        1
    };
    let split = Layout::vertical([Constraint::Length(title_rows), Constraint::Min(1)]).split(area);

    let mut source = if app.queue.smart_shuffle {
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
    if app.autoplay_epoch == Some(app.queue.epoch) {
        source.push_str(if app.autoplay_error.is_some() {
            " | Autoplay unavailable (R retries)"
        } else {
            " | Autoplay"
        });
    }

    // Keep the collection identity separate from the table's column labels.
    let title = if main {
        section_title(
            "Your queue",
            Some(format!("{} tracks", app.queue.ids.len())),
            theme,
        )
    } else {
        section_title("Queue", Some(app.queue.ids.len().to_string()), theme)
    };
    let title_area = Rect::new(
        split[0].x + 1,
        split[0].y + u16::from(title_rows >= 5),
        split[0].width.saturating_sub(2),
        1,
    );
    let source = source.trim_start_matches(" | ").to_owned();
    if main && title_rows >= 3 {
        let filter = " /  Filter queue ";
        let parts = Layout::horizontal([
            Constraint::Min(12),
            Constraint::Length(if title_area.width >= 40 {
                Span::raw(filter).width() as u16
            } else {
                0
            }),
        ])
        .split(title_area);
        frame.render_widget(
            Paragraph::new("Your queue").style(Style::default().fg(palette.text).bold()),
            parts[0],
        );
        frame.render_widget(Paragraph::new(filter).style(quiet_badge(theme)), parts[1]);
        hit(render, parts[1], MouseTarget::QueueFilter);
        let revision = (app.queue.revision, app.cache.revision);
        if render
            .queue_summary
            .as_ref()
            .is_none_or(|summary| summary.revision != revision)
        {
            let durations = app
                .queue
                .ids
                .iter()
                .filter_map(|id| app.cache.get(id))
                .map(|track| u64::from(track.duration_ms))
                .filter(|duration| *duration > 0);
            let (duration_ms, known_durations) =
                durations.fold((0, 0), |(sum, count), duration| (sum + duration, count + 1));
            render.queue_summary = Some(crate::app::ui_state::QueueSummary {
                revision,
                duration_ms,
                known_durations,
            });
        }
        let duration = render.queue_summary.as_ref().unwrap();
        let summary = format!(
            "{} tracks{}{}",
            app.queue.ids.len(),
            if duration.duration_ms > 0 {
                format!(
                    " · {} min{}",
                    duration.duration_ms / 60_000,
                    if duration.known_durations < app.queue.ids.len() {
                        " loaded"
                    } else {
                        ""
                    }
                )
            } else {
                String::new()
            },
            if source.is_empty() {
                String::new()
            } else {
                format!(" · {source}")
            }
        );
        frame.render_widget(
            Paragraph::new(summary).style(Style::default().fg(palette.text_muted)),
            Rect::new(title_area.x, title_area.y + 1, title_area.width, 1),
        );
    } else {
        let source_width = Span::raw(&source).width() as u16;
        let title_split =
            Layout::horizontal([Constraint::Min(8), Constraint::Length(source_width)])
                .split(title_area);
        frame.render_widget(Paragraph::new(title), title_split[0]);
        if !source.is_empty() && source_width + 12 < title_area.width {
            frame.render_widget(
                Paragraph::new(source).style(Style::default().fg(palette.text_subtle)),
                title_split[1],
            );
        }
    }

    let area = split[1];
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
            Paragraph::new(Line::from(vec![
                key_hint("Esc", theme, true),
                Span::styled(" Clear", Style::default().fg(palette.text_muted)),
            ]))
            .style(Style::default().bg(palette.surface_selected)),
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
    // Durations are right aligned inside their column, so the header and the
    // values share one edge.
    let time_width = if main {
        if area.width < 45 {
            0
        } else if area.width >= 60 {
            7
        } else {
            6
        }
    } else {
        5
    };
    if len == 0 {
        render.queue_height = area.height as usize;
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
                .style(Style::default().fg(palette.text_muted))
                .wrap(Wrap { trim: false }),
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
    // At portrait widths, the artist belongs underneath the title instead
    // of competing with it for two narrow columns.
    let stacked = main && area.width < 60 && area.height >= 12;
    let row_height = if stacked { 2 } else { 1 };
    let stride = if stacked {
        3
    } else if spacious && area.height >= 16 {
        2
    } else {
        1
    };
    let height = (area.height.saturating_sub(if compact {
        0
    } else if header {
        2
    } else {
        0
    }) / stride) as usize;
    render.queue_height = height;
    let visible = viewport(selected, len, height, &mut render.queue_scroll);
    if filtering && !visible.is_empty() {
        render.queue_filter_metadata_start = Some(filtered.as_ref().unwrap()[visible.start]);
    }
    let first_row = if header { 2 } else { 0 };
    for (line, row) in visible.clone().enumerate() {
        let at = filtered.as_ref().map_or(row, |rows| rows[row]);
        hit(
            render,
            Rect::new(
                area.x + 1,
                area.y + first_row + line as u16 * stride,
                area.width.saturating_sub(2),
                stride,
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
                    "► "
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

            let index = if main {
                if area.width < 60 {
                    format!("{indicator} {:>2}", at + 1)
                } else {
                    format!(" {indicator} {:>3} ", at + 1)
                }
            } else {
                indicator.to_owned()
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
            let name_style = if is_current {
                Style::default().fg(palette.primary).bold()
            } else if is_placeholder {
                Style::default().fg(palette.text_subtle).italic()
            } else {
                Style::default().fg(palette.text)
            };
            let name_cell = if stacked {
                Cell::from(Text::from(vec![
                    Line::from(Span::styled(name, name_style)),
                    Line::from(Span::styled(
                        artist,
                        Style::default().fg(palette.text_muted),
                    )),
                ]))
            } else {
                Cell::from(name).style(name_style)
            };

            if main {
                let artist_cell =
                    Cell::from(artist).style(Style::default().fg(if is_placeholder {
                        palette.text_subtle
                    } else {
                        palette.text_muted
                    }));
                let time_cell = Cell::from(format!("{duration:>time_width$}")).style(
                    Style::default().fg(if is_placeholder {
                        palette.text_subtle
                    } else {
                        palette.text_muted
                    }),
                );
                Row::new(vec![index_cell, name_cell, artist_cell, time_cell])
                    .height(row_height)
                    .bottom_margin(stride - row_height)
            } else {
                let time_cell = Cell::from(format!("{duration:>time_width$}")).style(
                    Style::default().fg(if is_placeholder {
                        palette.text_subtle
                    } else {
                        palette.text_muted
                    }),
                );
                Row::new(vec![index_cell, name_cell, time_cell])
            }
        })
        .collect();

    let mut state = TableState::default().with_selected(if main {
        Some(selected.saturating_sub(visible.start))
    } else {
        app.queue.cursor.map(|c| c.saturating_sub(visible.start))
    });

    if main {
        let widths = if stacked {
            vec![
                Constraint::Length(6),
                Constraint::Fill(1),
                Constraint::Length(0),
                Constraint::Length(time_width as u16),
            ]
        } else if area.width < 45 {
            vec![
                Constraint::Length(5),
                Constraint::Fill(1),
                Constraint::Length(0),
                Constraint::Length(0),
            ]
        } else if area.width >= 60 {
            vec![
                Constraint::Length(8),
                Constraint::Fill(3),
                Constraint::Fill(2),
                Constraint::Length(7),
            ]
        } else {
            vec![
                Constraint::Length(6),
                Constraint::Fill(3),
                Constraint::Fill(2),
                Constraint::Length(6),
            ]
        };
        let header_row = Row::new(vec![
            Cell::from("      # "),
            Cell::from(if stacked { "TRACK / ARTIST" } else { "TRACK" }),
            Cell::from(if stacked { "" } else { "ARTIST" }),
            Cell::from(format!("{:>time_width$}", "TIME")),
        ])
        .style(table_header_style(theme))
        .bottom_margin(1);

        let mut table = Table::new(rows, widths)
            .column_spacing(2)
            .block(panel_block(theme))
            .row_highlight_style(selected_row_style(theme))
            .highlight_symbol(selected_marker(theme));
        if header {
            table = table.header(header_row);
        }
        frame.render_stateful_widget(table, area, &mut state);
        // Ratatui applies a row highlight to its bottom margin too. Leave the
        // breathing room between tracks on the ordinary surface.
        if stride > 1 {
            let top = area.y + first_row;
            for line in 0..visible.len() {
                let y = top + line as u16 * stride + row_height;
                if y >= area.bottom().saturating_sub(1) {
                    break;
                }
                for x in area.x + 1..area.right().saturating_sub(1) {
                    frame.buffer_mut()[(x, y)]
                        .set_symbol(" ")
                        .set_style(Style::default().fg(palette.text).bg(palette.background));
                }
            }
        }
    } else {
        let widths = vec![
            Constraint::Length(2),
            Constraint::Fill(1),
            Constraint::Length(5),
        ];
        frame.render_stateful_widget(
            Table::new(rows, widths)
                .block(panel_block(theme))
                .row_highlight_style(selected_row_style(theme))
                .highlight_symbol(selected_marker(theme)),
            area,
            &mut state,
        );
    }
}
