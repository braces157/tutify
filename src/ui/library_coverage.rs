use super::*;

pub(super) fn library_coverage(
    frame: &mut Frame<'_>,
    app: &App,
    render: &mut RenderState,
    area: Rect,
) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let skipped = &app.catalog.library_skipped;
    let outer = block_themed(
        format!(" SKIPPED SOURCES · {} · F4 close ", skipped.len()),
        true,
        theme,
    );
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    if skipped.is_empty() {
        frame.render_widget(
            Paragraph::new(if app.catalog.busy {
                "Scanning; no skipped sources so far."
            } else {
                "No playlist sources were skipped."
            }),
            parts[0],
        );
    } else {
        let selected = app.ui.coverage_selected.min(skipped.len() - 1);
        let visible = viewport(
            selected,
            skipped.len(),
            parts[0].height as usize,
            &mut render.coverage_scroll,
        );
        let rows = visible.clone().map(|index| {
            let source = &skipped[index];
            // Names are bounded and control-free at the scanner boundary.
            let name = if source.name.is_empty() {
                &source.id
            } else {
                &source.name
            };
            ListItem::new(format!("{}. {name} · {}", index + 1, source.reason.label()))
        });
        let mut state =
            ListState::default().with_selected(Some(selected.saturating_sub(visible.start)));
        frame.render_stateful_widget(
            List::new(rows)
                .style(Style::default().fg(palette.text))
                .highlight_style(selected_row_style(theme))
                .highlight_symbol("▌ "),
            parts[0],
            &mut state,
        );
        for (row, index) in visible.enumerate() {
            hit(
                render,
                Rect::new(parts[0].x, parts[0].y + row as u16, parts[0].width, 1),
                MouseTarget::CoverageRow(index),
            );
        }
    }
    let controls = Layout::horizontal([Constraint::Length(12), Constraint::Min(1)]).split(parts[1]);
    frame.render_widget(
        Paragraph::new(" Esc close ").style(quiet_badge(theme)),
        controls[0],
    );
    frame.render_widget(
        Paragraph::new(" F5 recheck ").style(primary_badge(theme)),
        controls[1],
    );
    hit(render, controls[0], MouseTarget::CoverageClose);
    hit(render, controls[1], MouseTarget::CoverageRecheck);
}
