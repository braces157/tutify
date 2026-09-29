use super::*;
use crossterm::event::KeyCode;

pub(super) fn mix_builder(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let source = app
        .mix
        .source
        .as_ref()
        .map_or("No source", crate::mix::MixSource::label);
    let source_state = if app.mix.loading_source {
        " • LOADING/PARTIAL"
    } else if app.mix.source_partial {
        " • PARTIAL"
    } else {
        " • complete"
    };
    let outer = block_themed(
        format!(" MIX BUILDER • {source}{source_state} "),
        true,
        theme,
    );
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    if app.mix.naming {
        let actions = "Enter save • Esc keep name";
        let prompt = format!("Recipe name: {}_", app.mix.recipe_name);
        let action_height = Paragraph::new(actions)
            .wrap(Wrap { trim: true })
            .line_count(inner.width) as u16;
        let layout =
            Layout::vertical([Constraint::Min(1), Constraint::Length(action_height)]).split(inner);
        let scroll = Paragraph::new(prompt.as_str())
            .wrap(Wrap { trim: true })
            .line_count(inner.width)
            .saturating_sub(layout[0].height as usize)
            .min(u16::MAX as usize) as u16;
        frame.render_widget(
            Paragraph::new(prompt)
                .style(Style::default().fg(palette.text).bold())
                .wrap(Wrap { trim: true })
                .scroll((scroll, 0)),
            layout[0],
        );
        mix_control_hits(
            render,
            layout[1],
            actions,
            0,
            std::slice::from_ref(&(0..actions.len())),
            true,
        );
        frame.render_widget(
            Paragraph::new(actions)
                .style(Style::default().fg(palette.text).bold())
                .wrap(Wrap { trim: true }),
            layout[1],
        );
        return;
    }

    let achieved_minutes = (app.mix.preview.duration_ms + 30_000) / 60_000;
    let detail = selected_detail(app);
    if app.mix.detail {
        let heading = "MIX BUILDER DETAILS • ? back";
        let actions = "3/4/6 target • [/] suggestions • a artist gap\nUp/Down select or scroll • p pin • g regenerate/retry\nEnter replace • A append • w save • o reopen\n? close details • Esc close details, then cancel";
        let source_note = app
            .mix
            .source_error
            .as_deref()
            .unwrap_or("Source loaded without a reported error");
        let text = format!(
            "{heading}\nSource: {source}{source_state}\nSource note: {source_note}\nTarget: {} min; achieved: {achieved_minutes} min\nSuggestions: desired {}%; achieved {}%\nArtist gap preference: {} tracks\n\nSelected: {detail}\n{}\n\nCONTROLS\n{actions}",
            app.mix.settings.target_minutes,
            app.mix.settings.recommendation_percent,
            app.mix.preview.recommendation_percent,
            app.mix.settings.artist_gap,
            app.mix.preview.note,
        );
        mix_control_hits(
            render,
            inner,
            &text,
            app.mix.detail_scroll,
            &[0..heading.len(), text.len() - actions.len()..text.len()],
            true,
        );
        frame.render_widget(
            Paragraph::new(text)
                .style(Style::default().fg(palette.text_muted))
                .wrap(Wrap { trim: true })
                .scroll((app.mix.detail_scroll, 0)),
            inner,
        );
        return;
    }
    if inner.height <= 5 {
        let selected = app.mix.preview.entries.get(app.mix.selected);
        let marker = selected.map_or(" ", |entry| if entry.pinned { "◆" } else { " " });
        let track = selected.map_or("No preview track yet", |entry| entry.track.name.as_str());
        frame.render_widget(
            Paragraph::new(format!(
                "{marker} {} {track}\nEnter/A apply • p pin\n? details • Esc cancel",
                app.mix.selected.saturating_add(1)
            ))
            .style(Style::default().fg(palette.text).bold()),
            inner,
        );
        mix_control_hits(
            render,
            Rect::new(
                inner.x,
                inner.y.saturating_add(1),
                inner.width,
                inner.height.saturating_sub(1),
            ),
            "Enter/A apply • p pin\n? details • Esc cancel",
            0,
            std::slice::from_ref(&(0.."Enter/A apply • p pin\n? details • Esc cancel".len())),
            false,
        );
        return;
    }

    let loading = if app.mix.loading_recommendations {
        " • loading suggestions"
    } else {
        ""
    };
    let controls = if inner.width >= 60 {
        format!(
            "{}m target → {achieved_minutes}m | rec {}%/{}% | gap {} | {} tracks{loading}\n3/4/6 time | [/] rec | a artist gap\nEnter replace | A append | p pin | g regenerate\nw save | o reopen | ? details | Esc cancel",
            app.mix.settings.target_minutes,
            app.mix.preview.recommendation_percent,
            app.mix.settings.recommendation_percent,
            app.mix.settings.artist_gap,
            app.mix.preview.entries.len(),
        )
    } else {
        format!(
            "{}m→{achieved_minutes}m | rec {}%/{}% | gap {}\n3/4/6 time | [/] rec | a gap\nEnter replace | A append | p pin\ng regen | w save | o reopen\n? details | Esc cancel",
            app.mix.settings.target_minutes,
            app.mix.preview.recommendation_percent,
            app.mix.settings.recommendation_percent,
            app.mix.settings.artist_gap,
        )
    };
    let control_lines = Paragraph::new(controls.as_str())
        .wrap(Wrap { trim: true })
        .line_count(inner.width)
        .min(inner.height.saturating_sub(2) as usize)
        .max(1) as u16;
    let detail_height = if inner.height.saturating_sub(control_lines) >= 5 {
        2
    } else {
        1
    };
    let layout = Layout::vertical([
        Constraint::Length(control_lines),
        Constraint::Min(1),
        Constraint::Length(detail_height),
    ])
    .split(inner);
    mix_control_hits(
        render,
        layout[0],
        &controls,
        0,
        std::slice::from_ref(&(0..controls.len())),
        true,
    );
    frame.render_widget(
        Paragraph::new(controls)
            .style(Style::default().fg(palette.text_muted))
            .wrap(Wrap { trim: true }),
        layout[0],
    );

    let height = layout[1].height.saturating_sub(1) as usize;
    let start = app.mix.selected.saturating_sub(height.saturating_sub(1));
    for (line, index) in (start..app.mix.preview.entries.len())
        .take(height)
        .enumerate()
    {
        let y = layout[1].y.saturating_add(1).saturating_add(line as u16);
        if y < layout[1].bottom() {
            hit(
                render,
                Rect::new(layout[1].x, y, layout[1].width, 1),
                MouseTarget::MixRow(index),
            );
        }
    }
    let rows = app
        .mix
        .preview
        .entries
        .iter()
        .enumerate()
        .skip(start)
        .take(height.max(1))
        .map(|(index, entry)| {
            let kind = match entry.provenance {
                crate::mix::Provenance::Source { .. } => "SOURCE",
                crate::mix::Provenance::Recommendation { .. } => "SUGGEST",
            };
            let current = app.queue.current() == Some(entry.track.id.as_str());
            Row::new(vec![
                Cell::from(format!(
                    "{}{:>3}",
                    if current {
                        if app.state == State::Playing {
                            "►"
                        } else {
                            "Ⅱ"
                        }
                    } else if entry.pinned {
                        "◆"
                    } else {
                        " "
                    },
                    index + 1
                ))
                .style(Style::default().fg(if current {
                    palette.primary
                } else if entry.pinned {
                    palette.primary_soft
                } else {
                    palette.text_subtle
                })),
                Cell::from(entry.track.name.clone()).style(
                    Style::default()
                        .fg(if current {
                            palette.primary
                        } else {
                            palette.text
                        })
                        .bold(),
                ),
                Cell::from(entry.track.artists.clone())
                    .style(Style::default().fg(palette.text_muted)),
                Cell::from(kind).style(Style::default().fg(palette.text_subtle)),
                Cell::from(time(entry.track.duration_ms))
                    .style(Style::default().fg(palette.text_subtle)),
            ])
        })
        .collect::<Vec<_>>();
    let widths = if inner.width >= 76 {
        vec![
            Constraint::Length(5),
            Constraint::Percentage(38),
            Constraint::Percentage(30),
            Constraint::Length(9),
            Constraint::Length(6),
        ]
    } else {
        vec![
            Constraint::Length(5),
            Constraint::Fill(1),
            Constraint::Length(0),
            Constraint::Length(8),
            Constraint::Length(0),
        ]
    };
    let mut state = TableState::default().with_selected(
        (!app.mix.preview.entries.is_empty()).then_some(app.mix.selected.saturating_sub(start)),
    );
    frame.render_stateful_widget(
        Table::new(rows, widths)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(palette.border))
                    .title(" PREVIEW ")
                    .title_style(Style::default().fg(palette.text_subtle).bold()),
            )
            .row_highlight_style(selected_row_style(theme))
            .highlight_symbol(selected_marker(theme)),
        layout[1],
        &mut state,
    );
    let note = if app.mix.preview.note.is_empty() {
        detail
    } else {
        format!("{detail} • {} • ? details", app.mix.preview.note)
    };
    frame.render_widget(
        Paragraph::new(note)
            .style(Style::default().fg(palette.primary_soft))
            .wrap(Wrap { trim: true }),
        layout[2],
    );
}

fn mix_control_hits(
    render: &mut RenderState,
    area: Rect,
    controls: &str,
    scroll: u16,
    sections: &[std::ops::Range<usize>],
    wrap: bool,
) {
    if area.is_empty() {
        return;
    }
    let mut ranges = Vec::new();
    for section in sections {
        let text = &controls[section.clone()];
        for (label, code) in [
            ("Enter save", KeyCode::Enter),
            ("Esc keep name", KeyCode::Esc),
            ("? close details", KeyCode::Char('?')),
            ("? back", KeyCode::Char('?')),
            ("Esc close details", KeyCode::Esc),
            ("p pin", KeyCode::Char('p')),
            ("g regenerate", KeyCode::Char('g')),
            ("g regen", KeyCode::Char('g')),
            ("w save", KeyCode::Char('w')),
            ("o reopen", KeyCode::Char('o')),
            ("a artist gap", KeyCode::Char('a')),
            ("a gap", KeyCode::Char('a')),
            ("? details", KeyCode::Char('?')),
            ("Esc cancel", KeyCode::Esc),
        ] {
            for (start, _) in text.match_indices(label) {
                ranges.push((
                    section.start + start..section.start + start + label.len(),
                    code,
                ));
            }
        }
        for (start, _) in text.match_indices("3/4/6") {
            for (offset, code) in [(0, '3'), (2, '4'), (4, '6')] {
                let start = section.start + start + offset;
                ranges.push((start..start + 1, KeyCode::Char(code)));
            }
        }
        for (start, _) in text.match_indices("[/]") {
            for (offset, code) in [(0, '['), (2, ']')] {
                let start = section.start + start + offset;
                ranges.push((start..start + 1, KeyCode::Char(code)));
            }
        }
    }
    ranges.sort_by_key(|(range, _)| (range.start, std::cmp::Reverse(range.len())));
    let mut targets = Vec::new();
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in controls.split('\n') {
        let mut spans = Vec::new();
        let mut cursor = 0;
        for (range, code) in &ranges {
            if range.start < offset || range.end > offset + line.len() {
                continue;
            }
            let start = range.start - offset;
            let end = range.end - offset;
            if start < cursor {
                continue;
            } // Keep the longest overlapping label.
            spans.push(Span::raw(&line[cursor..start]));
            let marker = targets
                .iter()
                .position(|target| target == code)
                .unwrap_or_else(|| {
                    targets.push(*code);
                    targets.len() - 1
                });
            spans.push(Span::styled(
                &line[start..end],
                Style::default().bg(Color::Indexed(marker as u8)),
            ));
            cursor = end;
        }
        spans.push(Span::raw(&line[cursor..]));
        lines.push(Line::from(spans));
        offset += line.len() + 1;
    }
    // Use Ratatui's own reflow and clipping to locate controls. Marker colors
    // exist only in this scratch buffer; the user's paragraph stays unchanged.
    let mut buffer = Buffer::empty(area);
    let mut paragraph = Paragraph::new(lines).scroll((scroll, 0));
    if wrap {
        paragraph = paragraph.wrap(Wrap { trim: true });
    }
    paragraph.render(area, &mut buffer);
    for y in area.y..area.bottom() {
        let mut x = area.x;
        while x < area.right() {
            let Color::Indexed(marker) = buffer[(x, y)].bg else {
                x += 1;
                continue;
            };
            let start = x;
            while x < area.right() && buffer[(x, y)].bg == Color::Indexed(marker) {
                x += 1;
            }
            if let Some(code) = targets.get(marker as usize) {
                hit(
                    render,
                    Rect::new(start, y, x - start, 1),
                    MouseTarget::MixKey(*code),
                );
            }
        }
    }
}

fn selected_detail(app: &App) -> String {
    if app.mix.naming {
        return format!(
            "Recipe name: {}_ (Enter saves, Esc keeps name and cancels editing)",
            app.mix.recipe_name
        );
    }
    if let Some(entry) = app.mix.preview.entries.get(app.mix.selected) {
        return format!(
            "{} — {} — {}",
            entry.track.name,
            entry.track.artists,
            entry.provenance.explanation()
        );
    }
    if app.mix.loading_source {
        "Fetching source data asynchronously…".into()
    } else {
        "No usable source candidates yet.".into()
    }
}
