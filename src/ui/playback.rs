use super::*;

/// The transport state as a symbol plus a word.  The state never depends on
/// colour alone.
fn state_label(app: &App, palette: &Palette) -> (&'static str, Color) {
    match app.state {
        State::Playing => ("▶ PLAYING", palette.primary),
        State::Paused => ("|| PAUSED", palette.text_muted),
        State::Loading => ("… LOADING", palette.status_warning),
        State::Failed => ("! ERROR", palette.status_error),
    }
}

fn volume_label(app: &App) -> String {
    if app.config.volume == 0 {
        "VOL MUTED".into()
    } else {
        format!("VOL {}%", app.config.volume)
    }
}

fn shuffle_label(app: &App) -> String {
    if app.queue.smart_shuffle {
        "SHUF:SMART".into()
    } else if app.config.shuffle {
        "SHUF:ON".into()
    } else {
        "SHUF:OFF".into()
    }
}

fn repeat_label(app: &App) -> String {
    match app.config.repeat {
        Repeat::Off => "R:Off".to_owned(),
        Repeat::Queue => "R:Queue".to_owned(),
        Repeat::Track => "R:Track".to_owned(),
    }
}

fn action_label(app: &App) -> &'static str {
    if app.state == State::Playing {
        "Pause"
    } else {
        "Play"
    }
}

/// The sleep timer state, when one is running.
fn sleep_label(app: &App) -> Option<String> {
    app.ui.listening.sleep.label(std::time::Instant::now())
}

fn elapsed_and_duration(app: &App) -> (u32, u32) {
    let duration = app.current_track().as_ref().map_or(0, |t| t.duration_ms);
    let elapsed = if duration == 0 {
        app.queue.position_ms
    } else {
        app.queue.position_ms.min(duration)
    };
    (elapsed, duration)
}

/// The tall-window listening column: metadata, transport and seeking stay
/// together above the spectrum instead of stretching across a second panel.
pub(super) fn listening_player(
    frame: &mut Frame<'_>,
    app: &App,
    render: &mut RenderState,
    area: Rect,
) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    frame.render_widget(
        Block::default().style(Style::default().bg(palette.surface)),
        area,
    );
    column_rule(frame, Rect::new(area.x, area.y, 1, area.height), theme);
    let inner = Rect::new(
        area.x + 2,
        area.y,
        area.width.saturating_sub(4),
        area.height,
    );
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    let title = app
        .ui
        .listening
        .sleep
        .label(std::time::Instant::now())
        .map_or_else(
            || "NOW PLAYING".to_owned(),
            |timer| format!("NOW PLAYING · {timer}"),
        );
    frame.render_widget(
        Paragraph::new(Span::styled(
            title,
            Style::default().fg(palette.text_subtle),
        )),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    hit(
        render,
        Rect::new(inner.x, inner.y, inner.width, 1),
        MouseTarget::ListeningTools,
    );
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
    ));
    let track = app.current_track();
    frame.render_widget(
        Paragraph::new(
            track
                .as_ref()
                .map_or("Choose a track to start listening", |t| t.name.as_str()),
        )
        .style(Style::default().fg(palette.text).bold())
        .wrap(Wrap { trim: false }),
        parts[1],
    );
    frame.render_widget(
        Paragraph::new(
            track
                .as_ref()
                .map_or("/ Search for your next song", |t| t.artists.as_str()),
        )
        .style(Style::default().fg(palette.text_muted))
        .wrap(Wrap { trim: false }),
        parts[2],
    );
    let (state, color) = state_label(app, &palette);
    frame.render_widget(
        Paragraph::new(state).style(Style::default().fg(color).bold()),
        parts[3],
    );
    let (elapsed, duration) = elapsed_and_duration(app);
    let ratio = if duration == 0 {
        0.0
    } else {
        elapsed as f64 / duration as f64
    };
    let count = parts[4].width as usize;
    let filled = (ratio * count as f64) as usize;
    frame.render_widget(Paragraph::new(progress_bar(filled, count, theme)), parts[4]);
    hit(render, parts[4], MouseTarget::Seek);
    frame.render_widget(
        Paragraph::new(if duration == 0 {
            format!("{} / --:--", time(elapsed))
        } else {
            format!("{} / {}", time(elapsed), time(duration))
        })
        .style(Style::default().fg(palette.text_muted)),
        parts[5],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            key_cap("Space", theme),
            Span::styled(
                format!("  {}", action_label(app)),
                Style::default().fg(palette.primary).bold(),
            ),
        ])),
        parts[7],
    );
    hit(render, parts[7], MouseTarget::PlayPause);
    frame.render_widget(
        Paragraph::new(volume_label(app)).style(Style::default().fg(palette.text_muted)),
        parts[8],
    );
    let mut meta = vec![Span::styled(
        shuffle_label(app),
        Style::default().fg(if app.config.shuffle {
            palette.primary
        } else {
            palette.text_subtle
        }),
    )];
    meta.push(Span::styled(
        format!("  {}", repeat_label(app)),
        Style::default().fg(if app.config.repeat == Repeat::Off {
            palette.text_subtle
        } else {
            palette.primary
        }),
    ));
    frame.render_widget(Paragraph::new(Line::from(meta)), parts[9]);
}

/// The bottom band of wide windows.  The track that is sounding is the
/// loudest thing on the screen; the queue above it stays supporting context.
pub(super) fn playback(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    if area.height >= 5 && area.width >= 56 {
        expanded_playback(frame, app, render, area);
        return;
    }
    compact_playback(frame, app, render, area);
}

fn compact_playback(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    if area.height == 0 || area.width == 0 {
        return;
    }
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(area);
    let track = app.current_track();
    let (state, color) = state_label(app, &palette);

    let mut track_spans = vec![Span::styled(
        format!("{state}  "),
        Style::default().fg(color).bold(),
    )];
    if let Some(sleep) = sleep_label(app) {
        track_spans.push(Span::styled(
            format!("{sleep}  "),
            Style::default().fg(palette.primary_soft).bold(),
        ));
    }
    if app
        .queue
        .cursor
        .is_some_and(|c| app.queue.suggestions.contains(&app.queue.order[c]))
    {
        track_spans.push(Span::styled("✦ ", Style::default().fg(palette.primary)));
    }
    if let Some(t) = &track {
        track_spans.push(Span::styled(
            t.name.clone(),
            Style::default().fg(palette.text).bold(),
        ));
        if !t.artists.is_empty() && parts[0].width >= 48 {
            track_spans.push(Span::styled(
                format!("  ·  {}", t.artists),
                Style::default().fg(palette.text_muted),
            ));
        }
    } else {
        track_spans.push(Span::styled(
            "Choose a track to start listening",
            Style::default().fg(palette.text_muted),
        ));
    }
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
    frame.render_widget(Paragraph::new(Line::from(track_spans)), row[0]);
    hit(render, row[0], MouseTarget::ListeningTools);
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
            ctrl_spans.push(Span::styled("   ", Style::default()));
        }
        if area.width >= 70 {
            ctrl_spans.push(Span::styled(
                volume_label(app),
                Style::default().fg(palette.text).bold(),
            ));
            ctrl_spans.push(Span::styled("   ", Style::default()));
            ctrl_spans.push(Span::styled(
                shuffle_label(app),
                Style::default().fg(if app.config.shuffle {
                    palette.primary
                } else {
                    palette.text_subtle
                }),
            ));
            ctrl_spans.push(Span::styled("   ", Style::default()));
            ctrl_spans.push(Span::styled(
                repeat_label(app),
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

    let (elapsed, duration) = elapsed_and_duration(app);
    let ratio = if duration == 0 {
        0.0
    } else {
        elapsed as f64 / duration as f64
    };
    let percent = if duration == 0 {
        0
    } else {
        (elapsed as u64 * 100 / duration as u64).min(100)
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
        let count = progress[1].width as usize;
        let filled = (ratio * count as f64) as usize;
        frame.render_widget(
            Paragraph::new(progress_bar(filled, count, theme)),
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
    if area.height == 0 || area.width == 0 {
        return;
    }
    frame.render_widget(
        Block::default().style(Style::default().bg(palette.surface)),
        area,
    );
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(area);
    // A single hairline lifts the player off the list above it.
    rule(frame, parts[0], theme);

    let inner = Rect::new(
        parts[1].x + 2,
        parts[1].y,
        parts[1].width.saturating_sub(4),
        parts[1].height,
    );
    let track = app.current_track();

    // The transport control: a filled key cap instead of a nested box.
    let transport = Line::from(vec![
        key_cap("Space", theme),
        Span::styled(
            format!("  {}", action_label(app)),
            Style::default().fg(palette.primary).bold(),
        ),
    ]);
    let transport_width = transport.width() as u16;
    let title_split =
        Layout::horizontal([Constraint::Min(8), Constraint::Length(transport_width)]).split(inner);
    frame.render_widget(
        Paragraph::new(
            track
                .as_ref()
                .map_or("Choose a track to start listening", |t| t.name.as_str()),
        )
        .style(Style::default().fg(palette.text).bold()),
        title_split[0],
    );
    hit(render, title_split[0], MouseTarget::ListeningTools);
    frame.render_widget(
        Paragraph::new(transport).alignment(Alignment::Right),
        title_split[1],
    );
    hit(render, title_split[1], MouseTarget::PlayPause);

    // Artist on the left, transport facts on the right.
    let artist_area = Rect::new(
        parts[2].x + 2,
        parts[2].y,
        parts[2].width.saturating_sub(4),
        parts[2].height,
    );
    let (state, color) = state_label(app, &palette);
    let mut facts = vec![Span::styled(state, Style::default().fg(color).bold())];
    if let Some(sleep) = sleep_label(app) {
        facts.push(Span::styled(
            format!("   {sleep}"),
            Style::default().fg(palette.primary_soft).bold(),
        ));
    }
    if area.width >= 56 {
        facts.push(Span::styled(
            format!("   {}", volume_label(app)),
            Style::default().fg(palette.text),
        ));
    }
    if area.width >= 84
        && let Some(quality) = app.audio_quality.as_ref()
    {
        facts.push(Span::styled(
            format!("   {}", quality.label()),
            Style::default().fg(palette.text_subtle),
        ));
    }
    let facts_width = Line::from(facts.clone()).width() as u16;
    let artist_split = Layout::horizontal([Constraint::Min(8), Constraint::Length(facts_width)])
        .split(artist_area);
    frame.render_widget(
        Paragraph::new(
            track
                .as_ref()
                .map_or("/ Search for your next song", |t| t.artists.as_str()),
        )
        .style(Style::default().fg(palette.text_muted)),
        artist_split[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(facts)).alignment(Alignment::Right),
        artist_split[1],
    );

    // Progress: one continuous rule instead of a row of square blocks.
    let bar_area = Rect::new(
        parts[3].x + 2,
        parts[3].y,
        parts[3].width.saturating_sub(4),
        parts[3].height,
    );
    let (elapsed, duration) = elapsed_and_duration(app);
    let ratio = if duration == 0 {
        0.0
    } else {
        elapsed as f64 / duration as f64
    };
    let count = bar_area.width as usize;
    let filled = (ratio * count as f64) as usize;
    frame.render_widget(Paragraph::new(progress_bar(filled, count, theme)), bar_area);
    hit(render, bar_area, MouseTarget::Seek);

    // Times on the left, the remaining transport switches on the right.
    let times_area = Rect::new(
        parts[4].x + 2,
        parts[4].y,
        parts[4].width.saturating_sub(4),
        parts[4].height,
    );
    let percent = if duration == 0 {
        0
    } else {
        (u64::from(elapsed) * 100 / u64::from(duration)).min(100)
    };
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
    let switches = Line::from(vec![
        Span::styled(
            shuffle_label(app),
            Style::default().fg(if app.config.shuffle {
                palette.primary
            } else {
                palette.text_subtle
            }),
        ),
        Span::styled(
            format!("   {}", repeat_label(app)),
            Style::default().fg(if app.config.repeat == Repeat::Off {
                palette.text_subtle
            } else {
                palette.primary
            }),
        ),
    ]);
    let switches_width = switches.width() as u16;
    let times_split = Layout::horizontal([Constraint::Min(8), Constraint::Length(switches_width)])
        .split(times_area);
    frame.render_widget(
        Paragraph::new(details).style(Style::default().fg(palette.text_muted)),
        times_split[0],
    );
    frame.render_widget(
        Paragraph::new(switches).alignment(Alignment::Right),
        times_split[1],
    );
}
