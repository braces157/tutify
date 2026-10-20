use super::*;

/// Tall-window listening column: metadata, transport and seeking stay together
/// above the spectrum instead of stretching across a second full-width panel.
pub(super) fn listening_player(
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
            || " NOW PLAYING ".into(),
            |timer| format!(" NOW PLAYING · {timer} "),
        );
    let outer = block_themed(title, false, theme);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    hit(
        render,
        Rect::new(area.x + 1, area.y, area.width.saturating_sub(2), 1),
        MouseTarget::ListeningTools,
    );
    let parts = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(if inner.height >= 10 { 2 } else { 1 }),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(0),
    ])
    .split(inner);
    let track = app.current_track();
    frame.render_widget(
        Paragraph::new(
            track
                .as_ref()
                .map_or("Choose a track to start listening", |t| t.name.as_str()),
        )
        .style(Style::default().fg(palette.text).bold())
        .wrap(Wrap { trim: false }),
        parts[0],
    );
    frame.render_widget(
        Paragraph::new(
            track
                .as_ref()
                .map_or("/ Search for your next song", |t| t.artists.as_str()),
        )
        .style(Style::default().fg(palette.text_muted))
        .wrap(Wrap { trim: false }),
        parts[1],
    );
    let (state, color) = match app.state {
        State::Playing => ("PLAYING", palette.primary),
        State::Paused => ("PAUSED", palette.text_muted),
        State::Loading => ("LOADING", palette.status_warning),
        State::Failed => ("ERROR", palette.status_error),
    };
    let action = if app.state == State::Playing {
        "Pause"
    } else {
        "Play"
    };
    frame.render_widget(
        Paragraph::new(format!("{action} [Space] · {state}"))
            .style(Style::default().fg(color).bold()),
        parts[2],
    );
    hit(render, parts[2], MouseTarget::PlayPause);
    let duration = track.as_ref().map_or(0, |t| t.duration_ms);
    let elapsed = if duration == 0 {
        app.queue.position_ms
    } else {
        app.queue.position_ms.min(duration)
    };
    let count = parts[3].width as usize;
    let filled = if duration == 0 {
        0
    } else {
        (u64::from(elapsed) * count as u64 / u64::from(duration)) as usize
    };
    let bars = (0..count)
        .map(|index| {
            Span::styled(
                "■",
                Style::default().fg(if index < filled {
                    palette.primary
                } else {
                    palette.border
                }),
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Line::from(bars)), parts[3]);
    hit(render, parts[3], MouseTarget::Seek);
    let percent = if duration == 0 {
        0
    } else {
        u64::from(elapsed) * 100 / u64::from(duration)
    };
    let times = if duration == 0 {
        format!("{} / --:--", time(elapsed))
    } else {
        format!(
            "{} / {}  {percent}%  -{}",
            time(elapsed),
            time(duration),
            time(duration.saturating_sub(elapsed))
        )
    };
    frame.render_widget(
        Paragraph::new(times).style(Style::default().fg(palette.text_muted)),
        parts[4],
    );
    let volume = if app.config.volume == 0 {
        "VOL MUTED".into()
    } else {
        format!("VOL {}%", app.config.volume)
    };
    let shuffle = if app.queue.smart_shuffle {
        "SHUF:SMART"
    } else if app.config.shuffle {
        "SHUF:ON"
    } else {
        "SHUF:OFF"
    };
    let repeat = match app.config.repeat {
        Repeat::Off => "R:Off",
        Repeat::Queue => "R:Queue",
        Repeat::Track => "R:Track",
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(volume, Style::default().fg(palette.text))),
            Line::from(Span::styled(
                format!("{shuffle}   {repeat}"),
                Style::default().fg(palette.text_subtle),
            )),
        ]),
        parts[5],
    );
}

pub(super) fn playback(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    if area.height >= 6 && area.width >= 60 {
        expanded_playback(frame, app, render, area);
        return;
    }
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let title = app
        .ui
        .listening
        .sleep
        .label(std::time::Instant::now())
        .map_or_else(
            || " NOW PLAYING ".into(),
            |timer| format!(" NOW PLAYING · {timer} "),
        );
    let mut outer = block_themed(title, false, theme)
        .style(Style::default().fg(palette.text).bg(palette.surface_alt));
    if app.audio_quality.is_some() {
        outer = outer.title_bottom(
            Line::from(Span::styled(
                format!(" {} ", audio_badge(app, area.width)),
                Style::default().fg(palette.primary_soft).bold(),
            ))
            .alignment(Alignment::Right),
        );
    }
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    hit(
        render,
        Rect::new(area.x + 1, area.y, area.width.saturating_sub(2), 1),
        MouseTarget::ListeningTools,
    );
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

fn expanded_playback(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let title = app
        .ui
        .listening
        .sleep
        .label(std::time::Instant::now())
        .map_or_else(
            || " NOW PLAYING ".into(),
            |timer| format!(" NOW PLAYING · {timer} "),
        );
    let mut outer = block_themed(title, false, theme)
        .style(Style::default().fg(palette.text).bg(palette.surface));
    if app.audio_quality.is_some() {
        outer = outer.title_bottom(
            Line::from(Span::styled(
                format!(" {} ", audio_badge(app, area.width)),
                Style::default().fg(palette.primary_soft),
            ))
            .alignment(Alignment::Right),
        );
    }
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    hit(
        render,
        Rect::new(area.x + 1, area.y, area.width.saturating_sub(2), 1),
        MouseTarget::ListeningTools,
    );
    let track = app.current_track();
    let transport = Rect::new(inner.x + 1, inner.y, 9, inner.height);
    let action = if app.state == State::Playing {
        "Pause"
    } else {
        "Play"
    };
    let button = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette.primary_soft))
        .title_bottom(Line::from(" Space ").alignment(Alignment::Center));
    let button_inner = button.inner(transport);
    frame.render_widget(button, transport);
    frame.render_widget(
        Paragraph::new(action)
            .alignment(Alignment::Center)
            .style(Style::default().fg(palette.primary).bold()),
        Rect::new(
            button_inner.x,
            button_inner.y + button_inner.height.saturating_sub(1) / 2,
            button_inner.width,
            1,
        ),
    );
    hit(render, transport, MouseTarget::PlayPause);
    let content = Rect::new(
        transport.right() + 2,
        inner.y,
        inner.right().saturating_sub(transport.right() + 3),
        inner.height,
    );
    let top = Layout::horizontal([
        Constraint::Min(12),
        Constraint::Length(if area.width >= 80 { 35 } else { 12 }),
    ])
    .split(Rect::new(content.x, content.y, content.width, 1));
    frame.render_widget(
        Paragraph::new(
            track
                .as_ref()
                .map_or("Choose a track to start listening", |t| t.name.as_str()),
        )
        .style(Style::default().fg(palette.text).bold()),
        top[0],
    );
    let (state, color) = match app.state {
        State::Playing => ("▶ PLAYING", palette.primary),
        State::Paused => ("Ⅱ PAUSED", palette.text_muted),
        State::Loading => ("… LOADING", palette.status_warning),
        State::Failed => ("! ERROR", palette.status_error),
    };
    let volume = if app.config.volume == 0 {
        "VOL MUTED".into()
    } else {
        format!("VOL {}%", app.config.volume)
    };
    frame.render_widget(
        Paragraph::new(if area.width >= 80 {
            format!("{state}   {volume}")
        } else {
            state.into()
        })
        .style(Style::default().fg(color))
        .alignment(Alignment::Right),
        top[1],
    );
    let second = Layout::horizontal([
        Constraint::Min(10),
        Constraint::Length(if area.width >= 80 { 35 } else { 12 }),
    ])
    .split(Rect::new(content.x, content.y + 1, content.width, 1));
    frame.render_widget(
        Paragraph::new(
            track
                .as_ref()
                .map_or("Search / to find your next song", |t| t.artists.as_str()),
        )
        .style(Style::default().fg(palette.text_muted)),
        second[0],
    );
    let shuffle = if app.queue.smart_shuffle {
        "SHUF:SMART"
    } else if app.config.shuffle {
        "SHUF:ON"
    } else {
        "SHUF:OFF"
    };
    let repeat = match app.config.repeat {
        Repeat::Off => "R:Off",
        Repeat::Queue => "R:Queue",
        Repeat::Track => "R:Track",
    };
    frame.render_widget(
        Paragraph::new(if area.width >= 80 {
            format!("{shuffle}   {repeat}")
        } else {
            volume
        })
        .style(Style::default().fg(palette.text_subtle))
        .alignment(Alignment::Right),
        second[1],
    );
    hit(render, top[1], MouseTarget::PlayPause);
    let duration = track.as_ref().map_or(0, |t| t.duration_ms);
    let elapsed = if duration == 0 {
        app.queue.position_ms
    } else {
        app.queue.position_ms.min(duration)
    };
    let percent = if duration == 0 {
        0
    } else {
        (u64::from(elapsed) * 100 / u64::from(duration)).min(100)
    };
    let progress = Layout::horizontal([
        Constraint::Length(6),
        Constraint::Min(1),
        Constraint::Length(6),
    ])
    .split(Rect::new(content.x, content.y + 2, content.width, 1));
    frame.render_widget(
        Paragraph::new(time(elapsed)).style(Style::default().fg(palette.text)),
        progress[0],
    );
    frame.render_widget(
        Paragraph::new(if duration == 0 {
            "--:--".into()
        } else {
            time(duration)
        })
        .style(Style::default().fg(palette.text))
        .alignment(Alignment::Right),
        progress[2],
    );
    let count = progress[1].width as usize;
    let filled = if duration == 0 {
        0
    } else {
        (u64::from(elapsed) * count as u64 / u64::from(duration)) as usize
    };
    let spans = (0..count).map(|index| {
        Span::styled(
            "■",
            Style::default().fg(if index < filled {
                palette.primary
            } else {
                palette.border
            }),
        )
    });
    frame.render_widget(
        Paragraph::new(Line::from(spans.collect::<Vec<_>>())),
        progress[1],
    );
    hit(render, progress[1], MouseTarget::Seek);
    if content.height >= 4 {
        let details = if duration == 0 {
            format!("{} / --:--", time(elapsed))
        } else {
            format!(
                "{} / {}  {percent}%  -{}",
                time(elapsed),
                time(duration),
                time(duration.saturating_sub(elapsed))
            )
        };
        frame.render_widget(
            Paragraph::new(details).style(Style::default().fg(palette.text_subtle)),
            Rect::new(content.x, content.bottom() - 1, content.width, 1),
        );
    }
}
