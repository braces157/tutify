use super::*;

pub(super) fn stats(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let title = if area.width < 35 {
        " STATS [S/Esc exit] ".to_string()
    } else {
        " SONG STATISTICS [S/Esc exit] ".to_string()
    };
    let outer = block_themed(title, true, theme);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    if inner.width <= 2 || inner.height == 0 {
        return;
    }

    let mut view = app.ui.stats.borrow_mut();
    view.refresh(&app.stats);
    if app.stats.is_empty() {
        let empty_p = Paragraph::new(
            "\n  No song statistics yet.\n\n  • Play songs to build local listening statistics\n  • Press S or Esc to exit",
        )
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(palette.text_muted));
        frame.render_widget(empty_p, inner);
        return;
    }

    let overview_height = if inner.height >= 10 {
        4
    } else if inner.height >= 6 {
        2
    } else {
        0
    };
    let areas = Layout::vertical([
        Constraint::Length(overview_height),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(inner);
    let summary = vec![
        Line::from(vec![
            Span::styled("All time: ", Style::default().fg(palette.text_subtle)),
            Span::styled(
                format!(
                    "{} | {} plays | {} songs",
                    crate::stats::format_duration(view.total_ms),
                    view.total_plays,
                    view.unique_tracks
                ),
                Style::default().fg(palette.text).bold(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Most played: ", Style::default().fg(palette.text_subtle)),
            Span::styled(
                view.top_song.clone(),
                Style::default().fg(palette.primary_soft),
            ),
        ]),
        Line::from(Span::styled(
            "Local Tuitify playback only; Share = listening time %",
            Style::default().fg(palette.text_subtle),
        )),
    ];
    frame.render_widget(Paragraph::new(summary), areas[0]);
    let footer = if view.editing {
        format!("Search: {}_ [Enter done]", view.query)
    } else if !view.query.is_empty() {
        format!(
            "/ {} | {} matches | Tab: {} | Esc clear",
            view.query,
            view.rows.len(),
            view.sort.label()
        )
    } else {
        format!("/ Search | Tab sort: {} | S/Esc exit", view.sort.label())
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(palette.text_muted)),
        areas[2],
    );
    let inner = areas[1];
    let rows = &view.rows;
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new("No matching songs. / edit search | Esc clear")
                .wrap(Wrap { trim: false }),
            inner,
        );
        return;
    }
    let visible = viewport(
        view.selected,
        rows.len(),
        inner.height.saturating_sub(1) as usize,
        &mut render.stats_scroll,
    );

    let collapse_artist = inner.width < 70;
    let (widths, header) = if collapse_artist {
        (
            vec![
                Constraint::Length(5),
                Constraint::Min(10),
                Constraint::Length(7),
                Constraint::Length(9),
            ],
            Row::new(vec![
                Cell::from(" #").style(table_header_style(theme)),
                Cell::from("Title").style(table_header_style(theme)),
                Cell::from("Plays").style(table_header_style(theme)),
                Cell::from("Time").style(table_header_style(theme)),
            ]),
        )
    } else {
        (
            vec![
                Constraint::Length(5),
                Constraint::Min(10),
                Constraint::Percentage(25),
                Constraint::Length(7),
                Constraint::Length(10),
                Constraint::Length(6),
            ],
            Row::new(vec![
                Cell::from(" #").style(table_header_style(theme)),
                Cell::from("Title").style(table_header_style(theme)),
                Cell::from("Artist").style(table_header_style(theme)),
                Cell::from("Plays").style(table_header_style(theme)),
                Cell::from("Time").style(table_header_style(theme)),
                Cell::from("Share").style(table_header_style(theme)),
            ]),
        )
    };

    let table_rows: Vec<Row> = rows
        .iter()
        .enumerate()
        .skip(visible.start)
        .take(visible.len())
        .map(|(idx, stat)| {
            let is_selected = idx == view.selected;
            let current = app.queue.current() == Some(stat.id.as_str());
            let indicator = if current {
                if app.state == State::Playing {
                    "►"
                } else {
                    "||"
                }
            } else {
                " "
            };
            let rank_str = format!(
                "{}{:>2} ",
                if current {
                    indicator
                } else if is_selected {
                    "▌"
                } else {
                    " "
                },
                idx + 1
            );
            let rank_cell = Cell::from(rank_str).style(
                Style::default()
                    .fg(if current {
                        palette.primary
                    } else if is_selected {
                        palette.primary_soft
                    } else {
                        palette.text_subtle
                    })
                    .bold(),
            );
            let title_cell = Cell::from(stat.name.as_str()).style(
                Style::default()
                    .fg(if current {
                        palette.primary
                    } else {
                        palette.text
                    })
                    .bold(),
            );
            let plays_cell = Cell::from(format!("{:>5} ", stat.play_count))
                .style(Style::default().fg(palette.text_muted));
            let time_cell = Cell::from(format!(
                "{:>8} ",
                crate::stats::format_duration(stat.listened_ms)
            ))
            .style(Style::default().fg(palette.text_muted));

            let mut cells = vec![rank_cell, title_cell];
            if !collapse_artist {
                let artist_cell = Cell::from(stat.artists.as_str())
                    .style(Style::default().fg(palette.text_muted));
                cells.push(artist_cell);
            }
            cells.push(plays_cell);
            cells.push(time_cell);
            if !collapse_artist {
                let share = if view.total_ms == 0 {
                    0.0
                } else {
                    stat.listened_ms as f64 / view.total_ms as f64 * 100.0
                };
                cells.push(
                    Cell::from(format!("{share:.1}%"))
                        .style(Style::default().fg(palette.text_subtle)),
                );
            }

            let mut row = Row::new(cells);
            if is_selected {
                row = row.style(selected_row_style(theme));
            }
            row
        })
        .collect();

    let table = Table::new(table_rows, widths)
        .header(header)
        .column_spacing(1);
    frame.render_widget(table, inner);
}
