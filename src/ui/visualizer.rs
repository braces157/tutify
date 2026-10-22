use super::*;

/// The spectrum column.  Bars are drawn with half blocks so the analyser
/// reads as continuous columns instead of a grid of separate squares, and an
/// idle analyser shows a flat baseline rather than a dead field of dots.
pub(super) fn spectrum_panel(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    if area.height == 0 || area.width == 0 {
        return;
    }
    let inner = Rect::new(
        area.x + 2,
        area.y + 1,
        area.width.saturating_sub(4),
        area.height.saturating_sub(1),
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "SPECTRUM",
            Style::default().fg(palette.text_subtle),
        )),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    // One row is reserved for the baseline the bars stand on.
    let graph = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(5).min(12),
    );
    if graph.height < 2 || graph.width < 4 {
        return;
    }
    // Cap the graph height so supporting audio feedback does not overwhelm
    // the music library in a large window.
    const BAR_CELLS: usize = 1;
    let count = (usize::from(graph.width) / (BAR_CELLS + 1)).max(1);
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
    let idle = bars.iter().all(|bar| *bar == 0);
    let mut lines = Vec::with_capacity(height);
    if idle {
        // A flat baseline: the analyser is waiting, not broken.
        let mut spans = Vec::with_capacity(count);
        for _ in 0..count {
            spans.push(Span::styled(
                format!("{} ", "▁".repeat(BAR_CELLS)),
                Style::default().fg(palette.divider),
            ));
        }
        for _ in 1..height {
            lines.push(Line::default());
        }
        lines.push(Line::from(spans));
    } else {
        for row in 0..height {
            let mut spans = Vec::with_capacity(count);
            for col in 0..count {
                let bar = bars.get(col).copied().unwrap_or(0);
                let peak = peaks.get(col).copied().unwrap_or(0);
                let filled = bar > (height - 1 - row);
                let symbol = if filled {
                    format!("{} ", "█".repeat(BAR_CELLS))
                } else {
                    " ".repeat(BAR_CELLS + 1)
                };
                let color = if !filled {
                    palette.divider
                } else if peak == bar && bar == height - row {
                    palette.primary
                } else {
                    palette.text_subtle
                };
                spans.push(Span::styled(symbol, Style::default().fg(color)));
            }
            lines.push(Line::from(spans));
        }
    }
    frame.render_widget(Paragraph::new(lines), graph);
    // The baseline the bars stand on.
    frame.render_widget(
        Paragraph::new("─".repeat(usize::from(graph.width)))
            .style(Style::default().fg(palette.divider)),
        Rect::new(graph.x, graph.bottom(), graph.width, 1),
    );
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
        Rect::new(inner.x, graph.bottom() + 1, inner.width, 1),
    );
    let detail_y = graph.bottom() + 4;
    if inner.bottom().saturating_sub(detail_y) >= 6 {
        let detail = Rect::new(inner.x, detail_y, inner.width, inner.bottom() - detail_y);
        let rows = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(detail);
        frame.render_widget(
            Paragraph::new("IN YOUR QUEUE").style(Style::default().fg(palette.text_subtle)),
            rows[0],
        );
        if let Some(track) = app.current_track() {
            frame.render_widget(
                Paragraph::new(track.name.as_str())
                    .style(Style::default().fg(palette.text).bold())
                    .wrap(Wrap { trim: true }),
                rows[1],
            );
            frame.render_widget(
                Paragraph::new(track.artists.as_str())
                    .style(Style::default().fg(palette.text_muted))
                    .wrap(Wrap { trim: true }),
                rows[2],
            );
            if let Some(album) = track.album.as_deref() {
                frame.render_widget(
                    Paragraph::new(album)
                        .style(Style::default().fg(palette.text_subtle))
                        .wrap(Wrap { trim: true }),
                    rows[3],
                );
            }
        } else {
            frame.render_widget(
                Paragraph::new("Choose a track to start listening")
                    .style(Style::default().fg(palette.text_muted))
                    .wrap(Wrap { trim: true }),
                rows[1],
            );
        }
    }
}

/// A compact horizontal analyser for the docked column: one row of level
/// blocks that reads as a strip instead of a graph.
pub(super) fn spectrum_strip(frame: &mut Frame<'_>, app: &App, area: Rect) {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    const SIMULATED: [char; 8] = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    if area.height == 0 || area.width == 0 {
        return;
    }
    let inner = Rect::new(
        area.x + 2,
        area.y,
        area.width.saturating_sub(4),
        area.height,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("SPECTRUM", Style::default().fg(palette.text_subtle)),
            Span::styled("   v expand", Style::default().fg(palette.text_subtle)),
        ])),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let strip = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
    );
    if strip.height == 0 || strip.width == 0 {
        return;
    }
    let count = usize::from(strip.width);
    let playing = app.state == State::Playing;
    let levels: Vec<u8> = if app.visualizer.has_audio_samples() {
        let (bars, _) = app.visualizer.get_bars_and_peaks(count, 8, playing);
        bars.into_iter()
            .map(|bar| u8::try_from(bar.min(7)).unwrap_or(0))
            .collect()
    } else {
        generate_bars(app.animation_frame, count, playing)
            .chars()
            .map(|symbol| {
                SIMULATED
                    .iter()
                    .position(|block| *block == symbol)
                    .unwrap_or(0) as u8
            })
            .collect()
    };
    let spans: Vec<Span<'static>> = levels
        .into_iter()
        .map(|level| {
            let color = match level {
                0 => palette.divider,
                1..=4 => palette.primary_soft,
                _ => palette.primary,
            };
            Span::styled(
                BLOCKS[usize::from(level).min(7)].to_string(),
                Style::default().fg(color),
            )
        })
        .collect();
    frame.render_widget(Paragraph::new(Line::from(spans)), strip);
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
