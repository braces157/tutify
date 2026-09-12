use super::*;

pub fn generate_bars(frame: u32, count: usize, is_playing: bool) -> String {
    if !is_playing {
        return " ".repeat(count);
    }
    const BLOCKS: [char; 8] = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let mut s = String::with_capacity(count * 4);
    let t = frame as f64 * 0.16;
    for i in 0..count {
        let x = i as f64;
        let freq = 1.15 + (x * 0.18);
        let wave = (t * freq + x * 0.9).sin() * 0.42
            + (t * 0.65 - x * 0.45).cos() * 0.36
            + (t * 2.3 + x * 1.4).sin() * 0.22;
        let norm = ((wave + 1.0) * 0.5).clamp(0.0, 0.999);
        let idx = (norm * 8.0) as usize;
        s.push(BLOCKS[idx.min(7)]);
    }
    s
}

pub(super) fn block_themed(
    title: impl Into<Line<'static>>,
    focused: bool,
    theme: Theme,
) -> Block<'static> {
    let palette = theme.palette();
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .style(Style::default().fg(palette.text).bg(palette.surface))
        .border_style(Style::default().fg(if focused {
            palette.border_focus
        } else {
            palette.border
        }))
        .title_style(
            Style::default()
                .fg(if focused {
                    palette.primary
                } else {
                    palette.text_muted
                })
                .bold(),
        )
}

pub(super) fn selected_row_style(theme: Theme) -> Style {
    Style::default().bg(theme.palette().surface_selected)
}

pub(super) fn selected_marker(theme: Theme) -> Span<'static> {
    Span::styled("▌ ", Style::default().fg(theme.palette().primary))
}

pub(super) fn table_header_style(theme: Theme) -> Style {
    Style::default().fg(theme.palette().text_subtle).bold()
}

pub(super) fn key_hint(key: &'static str, theme: Theme, primary: bool) -> Span<'static> {
    let palette = theme.palette();
    Span::styled(
        format!(" {key} "),
        if primary {
            Style::default()
                .fg(palette.on_primary)
                .bg(palette.primary)
                .bold()
        } else {
            Style::default()
                .fg(palette.text_muted)
                .bg(palette.surface_alt)
                .bold()
        },
    )
}

pub(super) fn quiet_badge(theme: Theme) -> Style {
    let palette = theme.palette();
    Style::default()
        .fg(palette.text_muted)
        .bg(palette.surface_selected)
        .bold()
}

pub(super) fn primary_badge(theme: Theme) -> Style {
    let palette = theme.palette();
    Style::default()
        .fg(palette.on_primary)
        .bg(palette.primary)
        .bold()
}

pub(super) fn warning_badge(theme: Theme) -> Style {
    let palette = theme.palette();
    Style::default()
        .fg(palette.status_warning)
        .bg(palette.status_warning_bg)
        .bold()
}

pub(super) fn error_badge(theme: Theme) -> Style {
    let palette = theme.palette();
    Style::default()
        .fg(palette.status_error)
        .bg(palette.status_error_bg)
        .bold()
}
pub(super) fn time(ms: u32) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1000 % 60)
}
