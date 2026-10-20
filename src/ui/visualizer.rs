use super::*;

pub(super) fn spectrum_panel(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let outer = block_themed(" SPECTRUM ", false, theme);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    if inner.height < 3 || inner.width < 6 {
        return;
    }
    let graph = Rect::new(
        inner.x + 1,
        inner.y + 1,
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(3),
    );
    let count = graph.width.div_ceil(2) as usize;
    let height = graph.height as usize;
    let live = app.visualizer.has_audio_samples();
    let playing = app.state == State::Playing;
    let (bars, peaks) = if live {
        app.visualizer.get_bars_and_peaks(count, height, playing)
    } else {
        let bars: Vec<usize> = (0..count)
            .map(|col| {
                if !playing {
                    return 0;
                }
                let t = f64::from(app.animation_frame) * 0.16;
                let x = col as f64;
                let wave = (t + x * 0.62).sin() * 0.28 + (t * 0.7 - x * 0.35).cos() * 0.22;
                ((wave + 0.5) * height as f64)
                    .round()
                    .clamp(0.0, height as f64) as usize
            })
            .collect();
        (bars.clone(), bars)
    };
    let mut lines = Vec::new();
    for row in (1..=height).rev() {
        let mut spans = Vec::new();
        for col in 0..count {
            let lit = bars.get(col).copied().unwrap_or(0) >= row;
            let peak = peaks.get(col).copied().unwrap_or(0) == row;
            spans.push(Span::styled(
                if lit { "■ " } else { "· " },
                Style::default().fg(if peak || (lit && row > height / 2) {
                    palette.primary
                } else if lit {
                    palette.primary_soft
                } else {
                    palette.border
                }),
            ));
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(lines), graph);
    frame.render_widget(
        Paragraph::new(if live {
            "Live audio · v expand"
        } else if playing {
            "Standby · v expand"
        } else {
            "Paused · v expand"
        })
        .style(Style::default().fg(palette.text_subtle))
        .alignment(Alignment::Center),
        Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
    );
}

pub(super) fn visualizer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let outer = block_themed(" RETRO VISUALIZER [v exit] ", true, theme);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let parts = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(4),
        Constraint::Length(2),
    ])
    .split(inner);

    let track = app.current_track();
    let track_info = if let Some(t) = &track {
        format!("{} - {}", t.name, t.artists)
    } else {
        "No track playing".to_string()
    };
    let real_time = app.visualizer.has_audio_samples();
    let badge = if real_time {
        "[REAL-TIME SPECTRUM]"
    } else {
        "[STANDBY VISUALIZER]"
    };
    let available_track = (inner.width as usize).saturating_sub(16 + badge.len());
    let display_track = if track_info.chars().count() > available_track && available_track > 3 {
        format!(
            "{}...",
            track_info
                .chars()
                .take(available_track - 3)
                .collect::<String>()
        )
    } else {
        track_info
    };

    let status_line = Line::from(vec![
        Span::styled(
            if app.state == State::Playing {
                " ► PLAYING: "
            } else {
                " || PAUSED: "
            },
            Style::default().fg(palette.primary).bold(),
        ),
        Span::styled(display_track, Style::default().fg(palette.text).bold()),
        Span::styled(
            format!("   {badge}"),
            Style::default().fg(palette.text_muted).italic(),
        ),
    ]);
    frame.render_widget(Paragraph::new(status_line), parts[0]);

    let height = parts[1].height as usize;
    let width = parts[1].width as usize;
    if height >= 2 && width >= 10 {
        let is_playing = app.state == State::Playing;
        let bar_count = (width / 3).clamp(8, 32);

        let heights = if real_time {
            let (h, _) = app
                .visualizer
                .get_bars_and_peaks(bar_count, height, is_playing);
            h
        } else {
            let t = app.animation_frame as f64 * 0.16;
            (0..bar_count)
                .map(|col| {
                    if !is_playing {
                        return 0;
                    }
                    let x = col as f64;
                    let freq = 1.1 + x * 0.12;
                    let wave = (t * freq + x * 0.7).sin() * 0.42
                        + (t * 0.6 - x * 0.35).cos() * 0.35
                        + (t * 2.3 + x * 1.4).sin() * 0.23;
                    ((wave + 1.0) * 0.5 * height as f64).round() as usize
                })
                .collect()
        };

        let mut lines = Vec::new();
        for row in (1..=height).rev() {
            let mut spans = vec![Span::raw("  ")];
            for col in 0..bar_count {
                let val = heights.get(col).copied().unwrap_or(0);
                if val >= row {
                    let color = if row > height * 3 / 4 {
                        palette.primary
                    } else if row > height / 2 {
                        palette.primary_soft
                    } else {
                        palette.text_subtle
                    };
                    spans.push(Span::styled("█ ", Style::default().fg(color).bold()));
                } else {
                    spans.push(Span::styled("  ", Style::default()));
                }
            }
            lines.push(Line::from(spans));
        }
        frame.render_widget(Paragraph::new(lines), parts[1]);
    }

    let labels = if real_time {
        if inner.width >= 75 {
            Line::from(vec![
                Span::styled(
                    "  50Hz   120Hz   300Hz   800Hz   2kHz   4.5kHz   9kHz",
                    Style::default().fg(palette.primary_soft),
                ),
                Span::styled(" • Real-time FFT", Style::default().fg(palette.text_muted)),
            ])
        } else {
            Line::from(vec![
                Span::styled(
                    "  50Hz  200Hz  800Hz  2.5kHz  9kHz",
                    Style::default().fg(palette.primary_soft),
                ),
                Span::styled(" • Real-time FFT", Style::default().fg(palette.text_muted)),
            ])
        }
    } else {
        Line::from(vec![Span::styled(
            "  Waiting for live audio stream • press Space to play",
            Style::default().fg(palette.text_muted),
        )])
    };
    frame.render_widget(Paragraph::new(labels), parts[2]);
}
