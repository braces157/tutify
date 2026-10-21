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

/// A framed panel.  Reserved for overlays, modals and inputs, where the
/// reference contract asks for a clear visual boundary.
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

/// An unframed panel.  Content panels use this: structure comes from spacing
/// and tone steps instead of a box drawn around every region.
pub(super) fn panel_block(theme: Theme) -> Block<'static> {
    let palette = theme.palette();
    Block::default()
        .borders(Borders::NONE)
        .padding(Padding::horizontal(1))
        .style(Style::default().fg(palette.text))
}

/// A hairline rule that separates regions without drawing a box.
pub(super) fn rule(frame: &mut Frame<'_>, area: Rect, theme: Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new("─".repeat(usize::from(area.width)))
            .style(Style::default().fg(theme.palette().border)),
        Rect::new(area.x, area.y, area.width, 1),
    );
}

/// A vertical rule used to detach the navigation column from the content.
pub(super) fn column_rule(frame: &mut Frame<'_>, area: Rect, theme: Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new("│".repeat(usize::from(area.height)))
            .style(Style::default().fg(theme.palette().divider)),
        Rect::new(area.x, area.y, 1, area.height),
    );
}

/// The heading of an unframed panel: the subject in the brightest ink, the
/// supporting count beside it.
pub(super) fn section_title(title: &str, detail: Option<String>, theme: Theme) -> Line<'static> {
    let palette = theme.palette();
    let mut spans = vec![Span::styled(
        title.to_owned(),
        Style::default().fg(palette.text).bold(),
    )];
    if let Some(detail) = detail {
        spans.push(Span::styled(
            format!("  {detail}"),
            Style::default().fg(palette.text_subtle),
        ));
    }
    Line::from(spans)
}

pub(super) fn selected_row_style(theme: Theme) -> Style {
    Style::default().bg(theme.palette().surface_selected)
}

/// The non-colour marker that keeps the selected row identifiable when the
/// terminal cannot show the accent colour.
pub(super) fn selected_marker(theme: Theme) -> Span<'static> {
    Span::styled(
        selected_marker_text(theme),
        Style::default().fg(theme.palette().primary),
    )
}

/// The same marker as plain text, for widgets that only take a string.
pub(super) fn selected_marker_text(theme: Theme) -> &'static str {
    if theme == Theme::Glass {
        "› "
    } else {
        "▌ "
    }
}

pub(super) fn table_header_style(theme: Theme) -> Style {
    if theme == Theme::Glass {
        Style::default().fg(theme.palette().primary_soft).bold()
    } else {
        Style::default().fg(theme.palette().text_muted).bold()
    }
}

/// A key in a hint row.  The key carries the accent, the description that
/// follows it stays muted, so the hint row reads as a single quiet line.
pub(super) fn key_hint(key: &'static str, theme: Theme, primary: bool) -> Span<'static> {
    let palette = theme.palette();
    Span::styled(
        key.to_owned(),
        if primary {
            Style::default().fg(palette.primary).bold()
        } else {
            Style::default().fg(palette.primary_soft)
        },
    )
}

/// A filled key cap used for the single primary action of a panel.  It reads
/// as a button without drawing a border around it.
pub(super) fn key_cap(key: &str, theme: Theme) -> Span<'static> {
    let palette = theme.palette();
    Span::styled(
        format!(" {key} "),
        Style::default()
            .fg(palette.on_primary)
            .bg(palette.primary_soft)
            .bold(),
    )
}

/// A determinate progress bar drawn as a continuous rule: the elapsed part
/// carries the accent, the rest sits on the track colour.
pub(super) fn progress_bar(filled: usize, width: usize, theme: Theme) -> Line<'static> {
    let palette = theme.palette();
    let filled = filled.min(width);
    let mut spans = Vec::with_capacity(2);
    if filled > 0 {
        spans.push(Span::styled(
            "─".repeat(filled),
            Style::default().fg(palette.primary),
        ));
    }
    if filled < width {
        spans.push(Span::styled(
            "─".repeat(width - filled),
            Style::default().fg(palette.track),
        ));
    }
    Line::from(spans)
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
