use super::*;

pub(super) fn playback(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let outer = block_themed(" NOW PLAYING ", false, theme)
        .style(Style::default().fg(palette.text).bg(palette.surface_alt));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(inner);
    let track = app.current_track();

    let (badge_text, badge_style) = match app.state {
        State::Paused => (" Ⅱ PAUSED ", quiet_badge(theme)),
        State::Playing => (" ▶ PLAYING ", primary_badge(theme)),
        State::Loading => (" … LOADING ", warning_badge(theme)),
        State::Failed => (" ! ERROR ", error_badge(theme)),
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
        track_spans.push(Span::styled(" ✦", Style::default().fg(palette.primary)));
    }
    if let Some(t) = &track {
        track_spans.push(Span::styled(
            format!("  {}", t.name),
            Style::default().fg(palette.text).bold(),
        ));
        if !t.artists.is_empty() {
            track_spans.push(Span::styled(
                format!("  ·  {}", t.artists),
                Style::default().fg(palette.text_muted),
            ));
        }
    } else {
        track_spans.push(Span::styled(
            "  Choose a track to start listening",
            Style::default().fg(palette.text_muted),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(track_spans)), row[0]);
    hit(
        render,
        Rect::new(row[0].x, row[0].y, 10.min(row[0].width), row[0].height),
        MouseTarget::PlayPause,
    );
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
                Style::default().fg(palette.primary_soft).bold(),
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
            ctrl_spans.push(Span::styled(
                vol_str,
                Style::default().fg(palette.text).bold(),
            ));
            ctrl_spans.push(Span::styled("   ", Style::default()));
            ctrl_spans.push(Span::styled(
                s_str,
                Style::default().fg(if app.config.shuffle {
                    palette.primary
                } else {
                    palette.text_subtle
                }),
            ));
            ctrl_spans.push(Span::styled("   ", Style::default()));
            ctrl_spans.push(Span::styled(
                r_str,
                Style::default().fg(if app.config.repeat == Repeat::Off {
                    palette.text_subtle
                } else {
                    palette.primary
                }),
            ));
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
    if parts[1].width >= 48 {
        let label_width = (label.chars().count() as u16 + 2).min(parts[1].width.saturating_sub(8));
        let progress = Layout::horizontal([Constraint::Length(label_width), Constraint::Min(8)])
            .split(parts[1]);

        frame.render_widget(
            Paragraph::new(Span::styled(
                format!(" {label}"),
                Style::default().fg(palette.text_muted),
            )),
            progress[0],
        );
        frame.render_widget(
            Gauge::default()
                .ratio(ratio)
                .label(Span::raw(""))
                .use_unicode(true)
                .gauge_style(
                    Style::default()
                        .fg(palette.primary)
                        .bg(palette.surface_selected),
                ),
            progress[1],
        );
        hit(render, progress[1], MouseTarget::Seek);
    } else {
        frame.render_widget(
            Paragraph::new(Span::styled(label, Style::default().fg(palette.text_muted))),
            parts[1],
        );
    }
}
