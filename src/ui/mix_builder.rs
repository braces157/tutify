use super::*;

pub(super) fn mix_builder(frame: &mut Frame<'_>, app: &App, _render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
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
        frame.render_widget(
            Paragraph::new(format!(
                "Recipe name: {}_ • Enter save • Esc keep name",
                app.mix.recipe_name
            ))
            .style(Style::default().fg(theme.primary()).bold())
            .wrap(Wrap { trim: true })
            .scroll((0, 0)),
            inner,
        );
        return;
    }

    let achieved_minutes = (app.mix.preview.duration_ms + 30_000) / 60_000;
    let detail = selected_detail(app);
    if app.mix.detail {
        let source_note = app
            .mix
            .source_error
            .as_deref()
            .unwrap_or("Source loaded without a reported error");
        let text = format!(
            "MIX BUILDER DETAILS\nSource: {source}{source_state}\nSource note: {source_note}\nTarget: {} min; achieved: {achieved_minutes} min\nSuggestions: desired {}%; achieved {}%\nArtist gap preference: {} tracks\n\nSelected: {detail}\n{}\n\nCONTROLS\n3/4/6 target • [/] suggestions • a artist gap\nUp/Down select or scroll • p pin • g regenerate/retry\nEnter replace • A append • w save • o reopen\n? close details • Esc close details, then cancel",
            app.mix.settings.target_minutes,
            app.mix.settings.recommendation_percent,
            app.mix.preview.recommendation_percent,
            app.mix.settings.artist_gap,
            app.mix.preview.note,
        );
        frame.render_widget(
            Paragraph::new(text)
                .style(Style::default().fg(theme.accent_dim()))
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
            .style(Style::default().fg(theme.primary()).bold()),
            inner,
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
    frame.render_widget(
        Paragraph::new(controls)
            .style(Style::default().fg(MUTED))
            .wrap(Wrap { trim: true }),
        layout[0],
    );

    let height = layout[1].height.saturating_sub(1) as usize;
    let start = app.mix.selected.saturating_sub(height.saturating_sub(1));
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
            Row::new(vec![
                Cell::from(format!(
                    "{} {:>2}",
                    if entry.pinned { "◆" } else { " " },
                    index + 1
                )),
                Cell::from(entry.track.name.clone()),
                Cell::from(entry.track.artists.clone()),
                Cell::from(kind),
                Cell::from(time(entry.track.duration_ms)),
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
            .block(Block::default().borders(Borders::TOP).title(" PREVIEW "))
            .row_highlight_style(
                Style::default()
                    .fg(theme.primary())
                    .bg(theme.highlight_bg())
                    .bold(),
            ),
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
            .style(Style::default().fg(theme.accent_dim()))
            .wrap(Wrap { trim: true }),
        layout[2],
    );
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
