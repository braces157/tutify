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

pub(super) fn lyrics(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
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
    let outer = block_themed(title, true, theme);
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
        let p = Paragraph::new("\n  ⟳ Loading synchronized lyrics from Lrclib...")
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(palette.status_warning).italic());
        frame.render_widget(p, inner);
        return;
    }

    let Some(lyr) = &app.lyrics.content else {
        let p = Paragraph::new("\n  No lyrics available for this track.\n\n  • Press l to return to library view\n  • Songs with available lyrics will sync automatically")
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(palette.text_muted));
        frame.render_widget(p, inner);
        return;
    };

    if !lyr.lines.is_empty() {
        let active = lyr.current_line_index(app.queue.position_ms);
        let max_text_width = (inner.width as usize).saturating_sub(2).max(1);

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
        }

        let height = inner.height as usize;
        let half = height / 2;
        let scroll = active_visual_start.saturating_sub(half);
        let visible: Vec<Line<'static>> = all_lines.into_iter().skip(scroll).take(height).collect();
        frame.render_widget(Paragraph::new(visible), inner);
    } else if let Some(plain) = &lyr.plain {
        let p = Paragraph::new(plain.as_str())
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(palette.text));
        let max_scroll = p
            .line_count(inner.width)
            .saturating_sub(inner.height as usize);
        render.lyrics_length = max_scroll + 1;
        frame.render_widget(
            p.scroll((
                app.lyrics.scroll.min(max_scroll).min(u16::MAX as usize) as u16,
                0,
            )),
            inner,
        );
    } else {
        let p = Paragraph::new("\n  No lyrics found for this track. Press l to return.")
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(palette.text_muted));
        frame.render_widget(p, inner);
    }
}
