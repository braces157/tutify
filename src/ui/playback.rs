use super::*;

pub(super) fn playback(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let outer = block_themed(" NOW PLAYING ", false, theme);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let parts = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);
    let track = app.current_track();

    let (badge_text, badge_style) = match app.state {
        State::Paused => (
            " || PAUSED ",
            Style::default()
                .fg(theme.primary())
                .bg(theme.highlight_bg())
                .bold(),
        ),
        State::Playing => (
            " ► PLAYING ",
            Style::default()
                .fg(Color::Rgb(14, 17, 16))
                .bg(theme.primary())
                .bold(),
        ),
        State::Loading => (
            " ... LOAD ",
            Style::default()
                .fg(Color::Yellow)
                .bg(Color::Rgb(40, 35, 20))
                .bold(),
        ),
        State::Failed => (
            " ! ERROR ",
            Style::default()
                .fg(Color::Red)
                .bg(Color::Rgb(45, 20, 20))
                .bold(),
        ),
    };

    let (ctrl_width, bar_count) = if area.width >= 90 {
        (48, 10)
    } else if area.width >= 70 {
        (38, 6)
    } else if area.width >= 50 && app.state == State::Playing {
        (8, 6)
    } else {
        (0, 0)
    };

    let row =
        Layout::horizontal([Constraint::Min(12), Constraint::Length(ctrl_width)]).split(parts[0]);

    let mut track_spans = vec![Span::styled(badge_text, badge_style)];
    if app
        .queue
        .cursor
        .is_some_and(|c| app.queue.suggestions.contains(&app.queue.order[c]))
    {
        track_spans.push(Span::styled(" ✦", Style::default().fg(theme.primary())));
    }
    if let Some(t) = &track {
        track_spans.push(Span::styled(
            format!("  {}", t.name),
            Style::default().fg(FG).bold(),
        ));
        if !t.artists.is_empty() {
            track_spans.push(Span::styled(
                format!("  •  {}", t.artists),
                Style::default().fg(MUTED),
            ));
        }
    } else {
        track_spans.push(Span::styled(
            "  Choose a track to begin",
            Style::default().fg(MUTED),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(track_spans)), row[0]);
    hit(
        render,
        Rect::new(row[0].x, row[0].y, 10.min(row[0].width), row[0].height),
        MouseTarget::PlayPause,
    );
    hit(render, parts[1], MouseTarget::Seek);

    if ctrl_width > 0 {
        let mut ctrl_spans = Vec::new();
        if app.state == State::Playing && bar_count > 0 {
            let mini_bars = if app.visualizer.has_audio_samples() {
                app.visualizer.get_mini_bars(bar_count, true)
            } else {
                generate_bars(app.animation_frame, bar_count, true)
            };
            ctrl_spans.push(Span::styled(
                mini_bars,
                Style::default().fg(theme.primary()).bold(),
            ));
            if area.width >= 70 {
                ctrl_spans.push(Span::styled("   ", Style::default()));
            }
        }
        if area.width >= 70 {
            let vol_str = if app.config.volume == 0 {
                "VOL MUTED".to_owned()
            } else {
                format!("VOL {}%", app.config.volume)
            };
            let s_str = if app.queue.smart_shuffle {
                "SHUF:SMART"
            } else if app.config.shuffle {
                "SHUF:ON"
            } else {
                "SHUF:OFF"
            };
            let r_str = match app.config.repeat {
                Repeat::Off => "R:Off",
                Repeat::Queue => "R:Queue",
                Repeat::Track => "R:Track",
            };
            ctrl_spans.push(Span::styled(vol_str, Style::default().fg(FG).bold()));
            ctrl_spans.push(Span::styled("   ", Style::default()));
            ctrl_spans.push(Span::styled(
                s_str,
                Style::default().fg(if app.config.shuffle {
                    theme.primary()
                } else {
                    MUTED
                }),
            ));
            ctrl_spans.push(Span::styled("   ", Style::default()));
            ctrl_spans.push(Span::styled(r_str, Style::default().fg(FG).bold()));
            ctrl_spans.push(Span::styled(" ", Style::default()));
        }
        frame.render_widget(
            Paragraph::new(Line::from(ctrl_spans)).alignment(Alignment::Right),
            row[1],
        );
    }

    let duration = track.as_ref().map(|t| t.duration_ms).unwrap_or(0);
    let elapsed = app.queue.position_ms.min(duration);
    let ratio = if duration == 0 {
        0.0
    } else {
        elapsed as f64 / duration as f64
    };
    let label = if duration == 0 {
        format!(
            "{} / --:--{}",
            time(elapsed),
            if area.width < 70 {
                format!("  vol {}%", app.config.volume)
            } else {
                String::new()
            }
        )
    } else {
        let percent = (elapsed as u64 * 100 / duration as u64).min(100);
        format!(
            "{} / {}  {}%  -{}{}",
            time(elapsed),
            time(duration),
            percent,
            time(duration.saturating_sub(elapsed)),
            if area.width < 70 {
                format!("  vol {}%", app.config.volume)
            } else {
                String::new()
            }
        )
    };
    frame.render_widget(
        Gauge::default()
            .ratio(ratio)
            .label(Span::styled(
                label,
                Style::default().fg(Color::White).bold(),
            ))
            .use_unicode(true)
            .gauge_style(
                Style::default()
                    .fg(theme.primary())
                    .bg(theme.highlight_bg()),
            ),
        parts[1],
    );
}
