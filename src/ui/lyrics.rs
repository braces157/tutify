use super::*;

pub(super) fn wrap_lyric_line(text: &str, max_width: usize) -> Vec<String> {
    if text.trim().is_empty() {
        return vec![String::new()];
    }
    let max_width = max_width.max(1);
    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_len = 0;

    let push_word_chunked = |lines: &mut Vec<String>,
                             current_line: &mut String,
                             current_len: &mut usize,
                             word: &str| {
        let mut chunk = String::new();
        let mut chunk_width = 0;
        for c in word.chars() {
            let mut buf = [0u8; 4];
            let c_str = c.encode_utf8(&mut buf);
            let c_w = Span::raw(&*c_str).width();
            if chunk_width + c_w > max_width && !chunk.is_empty() {
                lines.push(chunk);
                chunk = String::new();
                chunk_width = 0;
            }
            chunk.push(c);
            chunk_width += c_w;
        }
        if !chunk.is_empty() {
            *current_line = chunk;
            *current_len = chunk_width;
        }
    };

    for word in text.split_whitespace() {
        let word_width = Span::raw(word).width();
        if current_line.is_empty() {
            if word_width <= max_width {
                current_line.push_str(word);
                current_len = word_width;
            } else {
                push_word_chunked(&mut lines, &mut current_line, &mut current_len, word);
            }
        } else if current_len + 1 + word_width <= max_width {
            current_line.push(' ');
            current_line.push_str(word);
            current_len += 1 + word_width;
        } else {
            lines.push(current_line);
            current_line = String::new();
            current_len = 0;

            if word_width <= max_width {
                current_line.push_str(word);
                current_len = word_width;
            } else {
                push_word_chunked(&mut lines, &mut current_line, &mut current_len, word);
            }
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

/// The synced lyric lines for the current playback position, styled the same
/// way for the full view and for the docked panel.  The second value is the
/// visual index of the line that is singing right now.
fn synced_lines(app: &App, max_text_width: usize) -> Option<(Vec<Line<'static>>, usize)> {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let lyr = app.lyrics.content.as_ref()?;
    if lyr.lines.is_empty() {
        return None;
    }
    let active = lyr.current_line_index(app.queue.position_ms);
    let mut all_lines: Vec<Line<'static>> = Vec::new();
    let mut active_visual_start = 0;

    for (i, l) in lyr.lines.iter().enumerate() {
        let is_active = Some(i) == active;
        let is_past = active.is_some_and(|act| i < act);
        let chunks = wrap_lyric_line(&l.text, max_text_width);

        if is_active {
            active_visual_start = all_lines.len();
        }

        for (chunk_idx, chunk) in chunks.into_iter().enumerate() {
            let prefix = if is_active && chunk_idx == 0 {
                Span::styled("► ", Style::default().fg(palette.primary).bold())
            } else {
                Span::raw("  ")
            };

            let text_span = if is_active {
                Span::styled(
                    chunk,
                    Style::default()
                        .fg(palette.text)
                        .bg(palette.surface_selected)
                        .bold(),
                )
            } else if is_past {
                Span::styled(chunk, Style::default().fg(palette.text_subtle))
            } else {
                Span::styled(chunk, Style::default().fg(palette.text_muted))
            };

            all_lines.push(Line::from(vec![prefix, text_span]));
        }
        if let Some(reading) = lyr
            .readings
            .get(i)
            .and_then(|r| r.get(app.lyrics.reading_mode, lyr.japanese))
        {
            let style = if is_active {
                Style::default()
                    .fg(palette.primary)
                    .bg(palette.surface_selected)
            } else if is_past {
                Style::default().fg(palette.text_subtle)
            } else {
                Style::default().fg(palette.text_muted)
            };
            for chunk in wrap_lyric_line(reading, max_text_width) {
                all_lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(chunk, style),
                ]));
            }
        }
    }

    Some((all_lines, active_visual_start))
}

/// The unsynced lyric lines, with their readings.
fn plain_lines(app: &App) -> Option<Vec<Line<'static>>> {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let lyr = app.lyrics.content.as_ref()?;
    let plain = lyr.plain.as_ref()?;
    let mut lines = Vec::new();
    for (i, original) in plain.lines().enumerate() {
        lines.push(Line::from(Span::styled(
            original.to_owned(),
            Style::default().fg(palette.text),
        )));
        if let Some(reading) = lyr
            .plain_readings
            .get(i)
            .and_then(|r| r.get(app.lyrics.reading_mode, lyr.japanese))
        {
            lines.push(Line::from(Span::styled(
                reading.to_owned(),
                Style::default().fg(palette.primary),
            )));
        }
    }
    Some(lines)
}

fn lyric_duration(app: &App) -> u32 {
    app.queue
        .current()
        .and_then(|id| app.cache.get(id))
        .map_or(0, |track| track.duration_ms)
}

fn scroll_label(app: &App) -> &'static str {
    if app.lyrics.manual_scroll {
        "off"
    } else if app
        .lyrics
        .content
        .as_ref()
        .is_some_and(|lyrics| !lyrics.lines.is_empty())
    {
        "on"
    } else if lyric_duration(app) > 0 {
        "estimated"
    } else {
        "no timing"
    }
}

fn scroll_offset(app: &App, max_scroll: usize, height: usize, active: Option<usize>) -> usize {
    if app.lyrics.manual_scroll {
        return app.lyrics.scroll.min(max_scroll);
    }
    if let Some(active) = active {
        return active.saturating_sub(height / 2).min(max_scroll);
    }
    let duration = lyric_duration(app);
    if duration == 0 {
        return 0;
    }
    // Untimed lyrics move through the wrapped viewport with song progress.
    // Keep this separate from real timestamps and never mark an estimated line
    // as the line being sung. Playback pause and seeking share this position.
    (max_scroll as u128 * u128::from(app.queue.position_ms.min(duration)) / u128::from(duration))
        as usize
}

pub(super) fn lyrics(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    render.lyrics_length = 1;
    render.lyrics_scroll = 0;
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let track = app.current_track();
    let title = if let Some(t) = &track {
        if area.width < 35 {
            " LYRICS [l exit] ".to_string()
        } else {
            let max_track = (area.width as usize).saturating_sub(22);
            if t.name.chars().count() > max_track && max_track > 2 {
                let truncated: String = t.name.chars().take(max_track.saturating_sub(1)).collect();
                format!(" LYRICS • {}… [l exit] ", truncated)
            } else {
                format!(" LYRICS • {} [l exit] ", t.name)
            }
        }
    } else {
        " LYRICS [l exit] ".to_string()
    };
    let reading_hint = if area.width >= 60 {
        format!(
            " a auto-scroll: {} | r readings: {} ",
            scroll_label(app),
            app.lyrics.reading_mode.label()
        )
    } else {
        format!(
            " a:{} r:{} ",
            scroll_label(app),
            app.lyrics.reading_mode.label()
        )
    };
    let outer =
        block_themed(title, true, theme).title_bottom(Line::from(reading_hint).right_aligned());
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    if inner.width <= 2 || inner.height == 0 {
        return;
    }

    if let Some(error) = &app.lyrics.error {
        frame.render_widget(
            Paragraph::new(error.as_str())
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(palette.status_error)),
            inner,
        );
        return;
    }
    if app.lyrics.loading {
        let p = Paragraph::new("\n  ⟳ Loading lyrics...")
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(palette.status_warning).italic());
        frame.render_widget(p, inner);
        return;
    }

    let max_text_width = (inner.width as usize).saturating_sub(2).max(1);
    if let Some((all_lines, active_visual_start)) = synced_lines(app, max_text_width) {
        let height = inner.height as usize;
        let max_scroll = all_lines.len().saturating_sub(height);
        let scroll = scroll_offset(app, max_scroll, height, Some(active_visual_start));
        render.lyrics_length = max_scroll + 1;
        render.lyrics_scroll = scroll;
        let visible: Vec<Line<'static>> = all_lines.into_iter().skip(scroll).take(height).collect();
        frame.render_widget(Paragraph::new(visible), inner);
    } else if let Some(lines) = plain_lines(app) {
        let p = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(palette.text));
        let max_scroll = p
            .line_count(inner.width)
            .saturating_sub(inner.height as usize);
        render.lyrics_length = max_scroll + 1;
        let scroll = scroll_offset(app, max_scroll, inner.height as usize, None);
        render.lyrics_scroll = scroll;
        frame.render_widget(p.scroll((scroll.min(u16::MAX as usize) as u16, 0)), inner);
    } else {
        let p = Paragraph::new("\n  No lyrics found for this track. Press l to return.")
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(palette.text_muted));
        frame.render_widget(p, inner);
    }
}

/// The docked lyrics column of tall windows: the singing line in the accent,
/// the lines around it quieter, and everything inside a single column so it
/// can live next to the player.
pub(super) fn lyrics_panel(frame: &mut Frame<'_>, app: &App, area: Rect) {
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
    let title = if app.lyrics.content.is_some() {
        let mode = match scroll_label(app) {
            "on" => "AUTO",
            "off" => "MANUAL",
            "estimated" => "ESTIMATED",
            _ => "NO TIMING",
        };
        format!("LYRICS · {mode}")
    } else {
        "LYRICS".into()
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            title,
            Style::default().fg(palette.text_subtle),
        )),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let body = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
    );
    if body.height == 0 || body.width == 0 {
        return;
    }
    if let Some(error) = &app.lyrics.error {
        frame.render_widget(
            Paragraph::new(error.as_str())
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(palette.status_error)),
            body,
        );
        return;
    }
    if app.lyrics.loading {
        frame.render_widget(
            Paragraph::new("⟳ Loading lyrics...")
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(palette.status_warning).italic()),
            body,
        );
        return;
    }

    let max_text_width = (body.width as usize).saturating_sub(2).max(1);
    if let Some((all_lines, active_visual_start)) = synced_lines(app, max_text_width) {
        let height = body.height as usize;
        let scroll = scroll_offset(
            app,
            all_lines.len().saturating_sub(height),
            height,
            Some(active_visual_start),
        );
        let visible: Vec<Line<'static>> = all_lines.into_iter().skip(scroll).take(height).collect();
        frame.render_widget(Paragraph::new(visible), body);
        return;
    }
    if let Some(lines) = plain_lines(app) {
        let p = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(palette.text));
        let max_scroll = p
            .line_count(body.width)
            .saturating_sub(body.height as usize);
        let scroll = scroll_offset(app, max_scroll, body.height as usize, None);
        frame.render_widget(p.scroll((scroll.min(u16::MAX as usize) as u16, 0)), body);
        return;
    }
    frame.render_widget(
        Paragraph::new("No lyrics for this track.\n\nL reloads · l opens the full view")
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(palette.text_muted)),
        body,
    );
}
