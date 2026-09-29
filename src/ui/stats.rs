use super::*;

pub(super) fn stats(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let title = if area.width < 35 {
        " STATS [S/Esc exit] ".to_string()
    } else {
        " SONG STATISTICS [S/Esc exit] ".to_string()
    };
    let title_area = Rect::new(
        area.x.saturating_add(1),
        area.y,
        area.width.saturating_sub(2),
        1,
    );
    hit_text(
        render,
        title_area,
        &title,
        "[S/Esc exit]",
        MouseTarget::StatsClose,
    );
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
    let compact = inner.width < 35;
    let sort_text = if view.query.is_empty() && !compact {
        format!("Tab sort: {}", view.sort.label())
    } else {
        format!("Tab: {}", view.sort.label())
    };
    let close_text = if compact { "Esc" } else { "S/Esc exit" };
    let footer = if view.editing {
        let query = fit_query(&view.query, areas[2].width.saturating_sub(22) as usize);
        format!("Search: {query}_ [Enter done]")
    } else if !view.query.is_empty() {
        let matches = if inner.width >= 60 {
            format!(" | {} matches", view.rows.len())
        } else {
            String::new()
        };
        let suffix = format!("{matches} | {sort_text} | Esc clear");
        let available = (areas[2].width as usize).saturating_sub(2 + Span::raw(&suffix).width());
        format!("/ {}{suffix}", fit_query(&view.query, available))
    } else {
        format!("/ Search | {sort_text} | {close_text}")
    };
    hit(render, areas[2], MouseTarget::StatsSearch);
    if view.editing {
        hit_text(
            render,
            areas[2],
            &footer,
            "[Enter done]",
            MouseTarget::StatsSearchDone,
        );
    } else {
        hit_text(
            render,
            areas[2],
            &footer,
            &sort_text,
            MouseTarget::StatsSort,
        );
        if view.query.is_empty() {
            hit_text(
                render,
                areas[2],
                &footer,
                close_text,
                MouseTarget::StatsClose,
            );
        } else {
            hit_text(
                render,
                areas[2],
                &footer,
                "Esc clear",
                MouseTarget::StatsClear,
            );
        }
    }
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
    for (line, index) in visible.clone().enumerate() {
        let y = inner.y.saturating_add(1).saturating_add(line as u16);
        if y < inner.bottom() {
            hit(
                render,
                Rect::new(inner.x, y, inner.width, 1),
                MouseTarget::StatsRow(index),
            );
        }
    }

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

fn fit_query(query: &str, width: usize) -> String {
    let span = Span::raw(query);
    if span.width() <= width {
        return query.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0;
    for grapheme in span.styled_graphemes(Style::default()) {
        let next = Span::raw(grapheme.symbol).width();
        if used + next > width - 1 {
            break;
        }
        result.push_str(grapheme.symbol);
        used += next;
    }
    result.push('…');
    result
}

fn hit_text(render: &mut RenderState, area: Rect, footer: &str, text: &str, target: MouseTarget) {
    let Some(start) = footer.rfind(text) else {
        return;
    };
    let x = area
        .x
        .saturating_add(ratatui::text::Span::raw(&footer[..start]).width() as u16);
    let width = ratatui::text::Span::raw(text)
        .width()
        .min(area.right().saturating_sub(x) as usize) as u16;
    hit(render, Rect::new(x, area.y, width, area.height), target);
}
