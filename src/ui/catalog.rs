use super::*;

pub(super) fn catalog(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let body = if app.catalog.view == View::Search {
        let split = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);
        let modes = [
            (SearchScope::Spotify, " F2  Spotify "),
            (SearchScope::Library, " F3  Your library "),
        ];
        let spans = modes
            .iter()
            .map(|(scope, label)| {
                Span::styled(
                    *label,
                    if app.catalog.search_scope == *scope {
                        Style::default()
                            .fg(palette.primary)
                            .bg(palette.surface_selected)
                            .bold()
                    } else {
                        Style::default().fg(palette.text_muted)
                    },
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(Line::from(spans)), split[0]);
        let mut x = split[0].x;
        for (scope, label) in modes {
            let width = (label.len() as u16).min(split[0].right().saturating_sub(x));
            hit(
                render,
                Rect::new(x, split[0].y, width, split[0].height),
                MouseTarget::SearchMode(scope),
            );
            x += width;
        }
        let prompt_line = if app.catalog.query.is_empty() && !app.catalog.editing {
            Line::from(vec![
                Span::styled("  / ", Style::default().fg(palette.primary).bold()),
                Span::styled(
                    if app.catalog.search_scope == SearchScope::Library {
                        "Search your saved songs and playlist tracks"
                    } else {
                        "Search songs, artists, or paste a Spotify track link"
                    },
                    Style::default().fg(palette.text_muted).italic(),
                ),
            ])
        } else {
            let visible = app
                .catalog
                .query
                .chars()
                .rev()
                .take(split[1].width.saturating_sub(6) as usize)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>();
            Line::from(vec![
                Span::styled("  › ", Style::default().fg(palette.primary).bold()),
                Span::styled(visible, Style::default().fg(palette.text).bold()),
                if app.catalog.editing {
                    Span::styled("▎", Style::default().fg(palette.primary))
                } else {
                    Span::raw("")
                },
            ])
        };
        frame.render_widget(
            Paragraph::new(prompt_line).block(block_themed(
                if app.catalog.editing {
                    " SEARCH  ·  Enter submit  ·  Esc cancel "
                } else {
                    app.catalog.search_scope.label()
                },
                app.catalog.editing,
                theme,
            )),
            split[1],
        );
        hit(render, split[1], MouseTarget::Prompt);
        split[2]
    } else if (app.catalog.view == View::Liked || app.catalog.view == View::Playlists)
        && (app.catalog.filtering || !app.catalog.filter.is_empty())
    {
        let split = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
        let prompt_line = if app.catalog.filter.is_empty() && !app.catalog.filtering {
            Line::from(vec![
                Span::styled("  / ", Style::default().fg(palette.primary).bold()),
                Span::styled(
                    "Press / or f to filter",
                    Style::default().fg(palette.text_muted).italic(),
                ),
            ])
        } else {
            let visible = app
                .catalog
                .filter
                .chars()
                .rev()
                .take(split[0].width.saturating_sub(8) as usize)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>();
            Line::from(vec![
                Span::styled("  › ", Style::default().fg(palette.primary).bold()),
                Span::styled(visible, Style::default().fg(palette.text).bold()),
                if app.catalog.filtering {
                    Span::styled("▎", Style::default().fg(palette.primary))
                } else {
                    Span::raw("")
                },
            ])
        };
        let filter_title = if app.catalog.filtering {
            " FILTER LOADED  ·  Enter apply  ·  Esc clear "
        } else {
            " FILTER LOADED  ·  F2 Spotify  ·  F3 Your library "
        };
        frame.render_widget(
            Paragraph::new(prompt_line).block(block_themed(
                filter_title,
                app.catalog.filtering,
                theme,
            )),
            split[0],
        );
        hit(render, split[0], MouseTarget::Prompt);
        split[1]
    } else {
        area
    };

    let title = if app.is_filtered() {
        let matched = app.filtered_indices().len();
        let total = app.raw_len();
        format!(
            " {} • {} of {} loaded matches{} ",
            app.catalog.title,
            matched,
            total,
            if app.catalog.busy {
                " • Loading..."
            } else {
                ""
            }
        )
    } else {
        format!(
            " {}{} ",
            app.catalog.title,
            if app.catalog.busy {
                " • Loading..."
            } else {
                ""
            }
        )
    };

    match &app.catalog.rows {
        Rows::Tracks(tracks) => {
            let indices = app.filtered_indices();
            let visible = viewport(
                app.catalog.selected,
                indices.len(),
                body.height.saturating_sub(4) as usize,
                &mut render.catalog_scroll,
            );
            row_hits(render, body, &visible, false, true);
            if tracks.is_empty() || (app.is_filtered() && indices.is_empty()) {
                let empty_msg = if app.catalog.busy {
                    "\n\n  Loading tracks…\n  Playback controls stay available while this finishes."
                } else if app.is_filtered() {
                    "\n  No loaded tracks match your filter.\n\n  • F3 searches all saved library tracks\n  • F2 searches Spotify\n  • Esc clears this loaded-page filter"
                } else {
                    "\n\n  Nothing here yet.\n\n  /  Search songs or paste a Spotify link\n  2  Browse playlists\n  3  Open liked songs"
                };
                frame.render_widget(
                    Paragraph::new(empty_msg)
                        .block(block_themed(title, !app.catalog.sidebar, theme))
                        .style(Style::default().fg(palette.text_muted))
                        .wrap(Wrap { trim: false }),
                    body,
                );
            } else {
                let rows: Vec<Row> = indices
                    .iter()
                    .enumerate()
                    .skip(visible.start)
                    .take(visible.len())
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
                                    .fg(if current {
                                        palette.primary
                                    } else {
                                        palette.text_subtle
                                    })
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
                                    palette.primary
                                } else if t.playable {
                                    palette.text
                                } else {
                                    palette.text_subtle
                                })
                                .bold(),
                        );
                        let artist_cell = Cell::from(t.artists.as_str()).style(
                            Style::default().fg(if t.playable {
                                palette.text_muted
                            } else {
                                palette.text_subtle
                            }),
                        );
                        let time_cell = Cell::from(if t.duration_ms > 0 {
                            time(t.duration_ms)
                        } else {
                            "--:--".to_string()
                        })
                        .style(Style::default().fg(palette.text_subtle));

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
                .style(table_header_style(theme))
                .bottom_margin(1);

                let mut state = TableState::default()
                    .with_selected(Some(app.catalog.selected.saturating_sub(visible.start)));
                frame.render_stateful_widget(
                    Table::new(rows, widths)
                        .header(header)
                        .block(block_themed(title, !app.catalog.sidebar, theme))
                        .row_highlight_style(selected_row_style(theme))
                        .highlight_symbol(selected_marker(theme)),
                    body,
                    &mut state,
                );
            }
        }
        Rows::Playlists(playlists) => {
            let indices = app.filtered_indices();
            let visible = viewport(
                app.catalog.selected,
                indices.len(),
                body.height.saturating_sub(4) as usize,
                &mut render.catalog_scroll,
            );
            row_hits(render, body, &visible, false, true);
            if playlists.is_empty() || (app.is_filtered() && indices.is_empty()) {
                let empty_msg = if app.catalog.busy {
                    "\n\n  Loading playlists…"
                } else if app.is_filtered() {
                    "\n  No playlists match your filter.\n\n  • Backspace to edit filter\n  • Esc to clear filter and show all playlists"
                } else {
                    "\n\n  No playlists found.\n  Press F5 to refresh or / to search."
                };
                frame.render_widget(
                    Paragraph::new(empty_msg)
                        .block(block_themed(title, !app.catalog.sidebar, theme))
                        .style(Style::default().fg(palette.text_muted))
                        .wrap(Wrap { trim: false }),
                    body,
                );
            } else {
                let rows: Vec<Row> = indices
                    .iter()
                    .enumerate()
                    .skip(visible.start)
                    .take(visible.len())
                    .filter_map(|(display_idx, &p_idx)| {
                        let p = playlists.get(p_idx)?;
                        let index_cell = Cell::from(format!("  {:>3}", display_idx + 1))
                            .style(Style::default().fg(palette.text_subtle));
                        let name_cell = Cell::from(p.name.as_str())
                            .style(Style::default().fg(palette.text).bold());
                        let owner_cell = Cell::from(p.owner.as_str())
                            .style(Style::default().fg(palette.text_muted));
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
                .style(table_header_style(theme))
                .bottom_margin(1);
                let mut state = TableState::default()
                    .with_selected(Some(app.catalog.selected.saturating_sub(visible.start)));
                frame.render_stateful_widget(
                    Table::new(rows, widths)
                        .header(header)
                        .block(block_themed(title, !app.catalog.sidebar, theme))
                        .row_highlight_style(selected_row_style(theme))
                        .highlight_symbol(selected_marker(theme)),
                    body,
                    &mut state,
                );
            }
        }
    }
}
