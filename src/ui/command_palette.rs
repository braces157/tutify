use super::*;

pub(super) fn draw(frame: &mut Frame<'_>, app: &App, render: &mut RenderState) {
    let Some(state) = &app.ui.palette else {
        return;
    };
    render.mouse_hits.clear();
    let screen = frame.area();
    let width = screen.width.min(76);
    let height = screen.height.min(18);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    let theme = Theme::from_str(&app.config.theme);
    let colors = theme.palette();
    frame.render_widget(Clear, area);
    let block = block_themed(" COMMANDS · Ctrl+P / F8 ", true, theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = crate::app::command_palette::matches(app);
    let selected = state.selected.min(rows.len().saturating_sub(1));
    let reason = rows.get(selected).map_or_else(
        || "Try lyrics, radio, sleep, shuffle".to_owned(),
        |spec| {
            spec.unavailable(app)
                .map_or_else(|| format!("Ready · {}", spec.shortcut), str::to_owned)
        },
    );
    let explanation = Paragraph::new(reason)
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(colors.text_muted));
    let explanation_height = explanation
        .line_count(inner.width)
        .min(inner.height.saturating_sub(3) as usize) as u16;
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(explanation_height),
        Constraint::Length(1),
    ])
    .split(inner);
    let query = Line::from(format!("> {}", state.query));
    let query_scroll = query.width().saturating_sub(parts[0].width as usize) as u16;
    frame.render_widget(
        Paragraph::new(query)
            .scroll((0, query_scroll))
            .style(Style::default().fg(colors.primary)),
        parts[0],
    );
    let visible = viewport(
        selected,
        rows.len(),
        parts[1].height as usize,
        &mut render.palette_scroll,
    );
    for (line, at) in visible.enumerate() {
        let spec = rows[at];
        let disabled = spec.unavailable(app).is_some();
        let row = Rect::new(parts[1].x, parts[1].y + line as u16, parts[1].width, 1);
        let style = if selected == at {
            selected_row_style(theme)
        } else {
            Style::default().fg(if disabled {
                colors.text_subtle
            } else {
                colors.text
            })
        };
        let shortcut = if width >= 50 { spec.shortcut } else { "" };
        let columns = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(shortcut.chars().count().min(24) as u16),
        ])
        .split(row);
        frame.render_widget(
            Paragraph::new(format!(
                "{}{}{}",
                if selected == at {
                    selected_marker_text(theme)
                } else {
                    "  "
                },
                if disabled { "× " } else { "" },
                spec.label
            ))
            .style(style),
            columns[0],
        );
        frame.render_widget(
            Paragraph::new(shortcut)
                .style(style)
                .alignment(Alignment::Right),
            columns[1],
        );
        hit(render, row, MouseTarget::PaletteRow(at));
    }
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new("No matching commands").style(Style::default().fg(colors.text_muted)),
            parts[1],
        );
    }
    frame.render_widget(explanation, parts[2]);
    let actions = Layout::horizontal([Constraint::Length(13), Constraint::Min(0)]).split(parts[3]);
    frame.render_widget(
        Paragraph::new(" Enter Run ").style(primary_badge(theme)),
        actions[0],
    );
    frame.render_widget(
        Paragraph::new(" Esc Cancel ").style(quiet_badge(theme)),
        actions[1],
    );
    hit(render, actions[0], MouseTarget::PaletteApply);
    hit(render, actions[1], MouseTarget::PaletteClose);
}
