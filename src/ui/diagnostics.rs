use super::*;
use crossterm::event::KeyCode;

pub(super) fn diagnostics(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let view = &app.ui.diagnostics;
    let title = if view.preview.is_some() {
        " SUPPORT REPORT · inspected snapshot "
    } else {
        " RECENT ERRORS · session only · F7 close "
    };
    let outer = block_themed(title, true, theme);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    if view.preview.is_some() {
        let paragraph = Paragraph::new(view.preview_json.clone()).wrap(Wrap { trim: false });
        let maximum = paragraph
            .line_count(parts[0].width)
            .saturating_sub(parts[0].height as usize);
        render.diagnostics_length = maximum + 1;
        frame.render_widget(
            paragraph.scroll((view.scroll.min(maximum).min(u16::MAX as usize) as u16, 0)),
            parts[0],
        );
    } else if view.history.records().is_empty() {
        frame.render_widget(Paragraph::new("No errors recorded in this session.\nSuccessful status and volume updates do not erase errors.\nPress r to inspect a redacted support report." ).wrap(Wrap { trim: false }), parts[0]);
    } else {
        let records = view.history.records();
        let selected = view.selected.min(records.len() - 1);
        let sections = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(if parts[0].height > 5 { 3 } else { 1 }),
        ])
        .split(parts[0]);
        let visible = viewport(
            selected,
            records.len(),
            sections[0].height as usize,
            &mut render.diagnostics_scroll,
        );
        let rows = visible.clone().map(|index| {
            let record = &records[records.len() - 1 - index];
            ListItem::new(format!(
                "#{} +{}s {}",
                record.sequence,
                record.elapsed_seconds,
                record.summary()
            ))
        });
        let mut state =
            ListState::default().with_selected(Some(selected.saturating_sub(visible.start)));
        frame.render_stateful_widget(
            List::new(rows)
                .style(Style::default().fg(palette.text))
                .highlight_style(selected_row_style(theme))
                .highlight_symbol("▌ "),
            sections[0],
            &mut state,
        );
        let record = &records[records.len() - 1 - selected];
        for (row, index) in visible.enumerate() {
            hit(
                render,
                Rect::new(
                    sections[0].x,
                    sections[0].y + row as u16,
                    sections[0].width,
                    1,
                ),
                MouseTarget::DiagnosticRow(index),
            );
        }
        let mut action = record.action().to_owned();
        if let Some(seconds) = record.retry_after_seconds {
            action.push_str(&format!(" Minimum wait recorded at failure: {seconds}s."));
        }
        frame.render_widget(
            Paragraph::new(action)
                .style(Style::default().fg(palette.text_muted))
                .wrap(Wrap { trim: false }),
            sections[1],
        );
    }
    let controls = Layout::horizontal([
        Constraint::Length(11),
        Constraint::Length(12),
        Constraint::Min(1),
    ])
    .split(parts[1]);
    for (area, text, code) in [
        (controls[0], " Esc close", KeyCode::Esc),
        (
            controls[1],
            if view.preview.is_some() {
                " r errors"
            } else {
                " r report"
            },
            KeyCode::Char('r'),
        ),
        (
            controls[2],
            if view.preview.is_some() {
                " e export"
            } else {
                " review to export"
            },
            KeyCode::Char('e'),
        ),
    ] {
        frame.render_widget(Paragraph::new(text).style(quiet_badge(theme)), area);
        if code != KeyCode::Char('e') || view.preview.is_some() {
            hit(render, area, MouseTarget::DiagnosticKey(code));
        }
    }
}
