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
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused {
            theme.primary()
        } else {
            theme.border_inactive()
        }))
        .title_style(
            Style::default()
                .fg(if focused { theme.primary() } else { MUTED })
                .bold(),
        )
}
pub(super) fn time(ms: u32) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1000 % 60)
}
