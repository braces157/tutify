use super::*;
use crate::app::listening::TOOL_LABELS;

pub(super) fn listening_tools(
    frame: &mut Frame<'_>,
    app: &App,
    render: &mut RenderState,
    area: Rect,
) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let title = app
        .ui
        .listening
        .sleep
        .label(std::time::Instant::now())
        .map_or_else(
            || " LISTENING TOOLS · F6 close ".into(),
            |timer| format!(" TOOLS · {timer} · F6 close "),
        );
    let outer = block_themed(title, true, theme);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    let selected = app.ui.listening.selected.min(TOOL_LABELS.len() - 1);
    let visible = viewport(
        selected,
        TOOL_LABELS.len(),
        parts[0].height as usize,
        &mut render.tools_scroll,
    );
    let items = visible
        .clone()
        .map(|index| ListItem::new(TOOL_LABELS[index]));
    let mut state =
        ListState::default().with_selected(Some(selected.saturating_sub(visible.start)));
    frame.render_stateful_widget(
        List::new(items)
            .style(Style::default().fg(palette.text))
            .highlight_style(selected_row_style(theme))
            .highlight_symbol("▌ "),
        parts[0],
        &mut state,
    );
    for (row, index) in visible.enumerate() {
        if row >= parts[0].height as usize {
            break;
        }
        hit(
            render,
            Rect::new(parts[0].x, parts[0].y + row as u16, parts[0].width, 1),
            MouseTarget::ListeningRow(index),
        );
    }
    let actions = Layout::horizontal([Constraint::Length(15), Constraint::Min(1)]).split(parts[1]);
    frame.render_widget(
        Paragraph::new(" Enter Apply ").style(primary_badge(theme)),
        actions[0],
    );
    frame.render_widget(
        Paragraph::new(" Esc Close ").style(quiet_badge(theme)),
        actions[1],
    );
    hit(render, actions[0], MouseTarget::ListeningApply);
    hit(render, actions[1], MouseTarget::ListeningClose);
}
