use super::*;

pub(super) fn header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let version = if app.demo {
        format!("v{} / demo", env!("CARGO_PKG_VERSION"))
    } else if app.config.deezer_search {
        format!("v{} / Deezer", env!("CARGO_PKG_VERSION"))
    } else if app.config.source == crate::model::MusicSource::Youtube && app.config.youtube_music {
        format!("v{} / YouTube Music", env!("CARGO_PKG_VERSION"))
    } else if app.config.source == crate::model::MusicSource::Youtube {
        format!("v{} / YouTube", env!("CARGO_PKG_VERSION"))
    } else if app.config.source == crate::model::MusicSource::Volta {
        format!("v{} / Volta", env!("CARGO_PKG_VERSION"))
    } else if app.config.source == crate::model::MusicSource::SpotifyVolta {
        format!("v{} / Spotify + Volta", env!("CARGO_PKG_VERSION"))
    } else {
        format!("v{}", env!("CARGO_PKG_VERSION"))
    };
    if area.height == 0 || area.width == 0 {
        return;
    }
    // The title bar is a tone step, not a box: the band itself separates it
    // from the content below.
    frame.render_widget(
        Block::default().style(Style::default().bg(palette.surface)),
        area,
    );
    let row = Rect::new(area.x + 1, area.y, area.width.saturating_sub(2), 1);
    let available = usize::from(row.width);

    // Right-hand facts, most informative first; the longest that fits wins.
    // The quit hint is a universal affordance and rides along when it fits.
    let source = app.audio_source_label();
    let quality = app.audio_quality.as_ref().map(|q| q.label().to_owned());
    // The theme is only worth naming when it is not already the source word.
    let theme_fact = if theme.name() == source {
        String::new()
    } else {
        format!("{} theme", theme.name())
    };
    let mut candidates: Vec<String> = Vec::new();
    if let Some(quality) = quality.as_ref() {
        let with_theme = format!("{source} · {quality}   {theme_fact}");
        candidates.push(with_theme.trim_end().to_owned());
        candidates.push(format!("{source} · {quality}"));
        candidates.push(quality.clone());
    }
    let source_with_theme = format!("{source}   {theme_fact}");
    candidates.push(source_with_theme.trim_end().to_owned());
    candidates.push(source.to_owned());
    if !theme_fact.is_empty() {
        candidates.push(theme_fact);
    }
    let fits = |text: &str| text.chars().count() + 9 <= available;
    let facts = candidates
        .iter()
        .find(|candidate| fits(candidate))
        .cloned()
        .unwrap_or_default();
    let with_quit = format!("{facts}   q Quit");
    let right = if !facts.is_empty() && fits(&with_quit) {
        with_quit
    } else if facts.is_empty() && fits("q Quit") {
        "q Quit".to_owned()
    } else {
        facts
    };

    let wordmark = "TUITIFY";
    let version_text = format!("  {version}");
    let show_version =
        wordmark.chars().count() + version_text.chars().count() + right.chars().count() + 3
            <= available;

    let parts = Layout::horizontal([
        Constraint::Min(8),
        Constraint::Length(right.chars().count() as u16),
    ])
    .split(row);
    let mut identity = vec![Span::styled(
        wordmark,
        Style::default().fg(palette.primary).bold(),
    )];
    if show_version {
        identity.push(Span::styled(
            version_text,
            Style::default().fg(palette.text_subtle),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(identity)), parts[0]);
    if !right.is_empty() {
        frame.render_widget(
            Paragraph::new(right).style(Style::default().fg(palette.text_muted)),
            parts[1],
        );
    }
}

/// The right-hand context of the status row: where the user is and how much
/// is loaded.
fn footer_context(app: &App) -> String {
    let view = app.catalog.view.name();
    if app.catalog.view == View::Queue {
        return format!("{view} · {} tracks", app.queue.ids.len());
    }
    let loaded = app.raw_len();
    if loaded > 0 {
        format!("{view} · {loaded}")
    } else {
        view.to_owned()
    }
}

pub(super) fn footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    if area.height == 0 || area.width == 0 {
        return;
    }
    if app.ui.overlay == Overlay::Diagnostics {
        let view = &app.ui.diagnostics;
        let message = if view.notice.is_empty() {
            "Session errors survive status changes; reports are never uploaded automatically."
        } else {
            view.notice
        };
        let filename = view
            .exported_name
            .as_ref()
            .map_or_else(String::new, |name| format!(" {name}"));
        frame.render_widget(
            Block::default().style(Style::default().bg(palette.surface)),
            area,
        );
        frame.render_widget(
            Paragraph::new(format!("{message}{filename}"))
                .style(Style::default().fg(palette.text_muted))
                .wrap(Wrap { trim: true }),
            Rect::new(
                area.x + 1,
                area.y,
                area.width.saturating_sub(2),
                area.height,
            ),
        );
        return;
    }
    let status_split = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    let auth_expired = app.catalog_health == crate::catalog::Health::AuthenticationRequired;

    let status_line = if auth_expired {
        Line::from(vec![
            Span::styled(" ! AUTH EXPIRED ", warning_badge(theme)),
            Span::styled(
                if app.config.source == crate::model::MusicSource::Youtube {
                    "  Google library connection expired. F6 > Connect Google music library."
                } else {
                    "  Spotify login expired. F6 > Connect Spotify account."
                },
                Style::default().fg(palette.text_muted),
            ),
        ])
    } else if app.state == State::Failed {
        Line::from(vec![
            Span::styled(" ! ERROR ", error_badge(theme)),
            Span::styled(
                format!("  {}", app.status),
                Style::default().fg(palette.status_error),
            ),
        ])
    } else if app.catalog.busy {
        Line::from(vec![
            Span::styled(" … ", Style::default().fg(palette.status_warning).bold()),
            Span::styled(
                format!("  {}", app.status),
                Style::default().fg(palette.status_warning),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("● ", Style::default().fg(palette.primary_soft)),
            Span::styled(
                format!(" {}", app.status),
                Style::default().fg(palette.text_muted),
            ),
        ])
    };

    frame.render_widget(
        Block::default().style(Style::default().bg(palette.surface)),
        area,
    );
    let context = footer_context(app);
    let status_width = status_line.width() as u16;
    let context_width = context.chars().count() as u16;
    let status_area = if status_width + context_width + 2 <= area.width {
        let parts = Layout::horizontal([Constraint::Min(4), Constraint::Length(context_width)])
            .split(status_split[0]);
        frame.render_widget(
            Paragraph::new(context).style(Style::default().fg(palette.text_subtle)),
            parts[1],
        );
        parts[0]
    } else {
        status_split[0]
    };
    frame.render_widget(
        Paragraph::new(status_line).wrap(Wrap { trim: true }),
        Rect::new(
            status_area.x + 1,
            status_area.y,
            status_area.width.saturating_sub(1),
            status_area.height,
        ),
    );

    if status_split[1].height == 0 {
        return;
    }
    let mut hints = Vec::new();
    if app.ui.overlay == Overlay::ListeningTools {
        push_hint(&mut hints, "Enter", "Apply", true, theme);
        push_hint(&mut hints, "↑/↓", "Choose", false, theme);
        push_hint(&mut hints, "Esc", "Close", false, theme);
    } else if app.ui.overlay == Overlay::MixBuilder {
        push_hint(&mut hints, "Enter", "Replace", true, theme);
        push_hint(&mut hints, "A", "Append", false, theme);
        push_hint(&mut hints, "p", "Pin", false, theme);
        if area.width >= 70 {
            push_hint(&mut hints, "g", "Regenerate", false, theme);
        }
        push_hint(&mut hints, "Esc", "Cancel", false, theme);
    } else if app.catalog.view == View::Queue
        && app.ui.overlay == Overlay::None
        && app.ui.queue.editing
    {
        push_hint(&mut hints, "Enter", "Done", true, theme);
        push_hint(&mut hints, "Esc", "Clear", false, theme);
        push_hint(&mut hints, "↑/↓", "Matches", false, theme);
    } else {
        push_hint(&mut hints, "Space", "Play/Pause", true, theme);
        if !app.catalog.history.is_empty() {
            push_hint(&mut hints, "Esc", "Back", false, theme);
        }
        push_hint(
            &mut hints,
            "/",
            if matches!(
                app.catalog.view,
                View::Liked | View::Playlists | View::Queue
            ) {
                "Filter"
            } else {
                "Search"
            },
            false,
            theme,
        );
        push_hint(&mut hints, "Tab", "Focus", false, theme);
        if docked_column(frame.area()) {
            push_hint(
                &mut hints,
                "L",
                if app.ui.lyrics_panel {
                    "Hide lyrics"
                } else {
                    "Lyrics"
                },
                false,
                theme,
            );
        }
        if area.width >= 80 {
            push_hint(&mut hints, "1–5", "Views", false, theme);
        }
        if area.width >= 100 {
            push_hint(&mut hints, "n/p", "Next/Prev", false, theme);
            push_hint(&mut hints, "v", "Spectrum", false, theme);
            push_hint(&mut hints, "F6", "Tools", false, theme);
        }
        if area.width >= 140 {
            push_hint(&mut hints, "t", "Theme", false, theme);
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(hints)),
        Rect::new(
            status_split[1].x + 1,
            status_split[1].y,
            status_split[1].width.saturating_sub(1),
            status_split[1].height,
        ),
    );
}

fn push_hint(
    spans: &mut Vec<Span<'static>>,
    key: &'static str,
    label: &'static str,
    primary: bool,
    theme: Theme,
) {
    spans.push(key_hint(key, theme, primary));
    spans.push(Span::styled(
        format!(" {label}   "),
        Style::default().fg(theme.palette().text_muted),
    ));
}

pub(super) fn context_menu(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    if let Some(menu) = &app.context_menu {
        let width = 28.min(area.width);
        let height = (menu.labels().len() as u16 + 2).min(area.height);
        let rect = Rect::new(
            menu.x.min(area.right().saturating_sub(width)).max(area.x),
            menu.y.min(area.bottom().saturating_sub(height)).max(area.y),
            width,
            height,
        );
        frame.render_widget(Clear, rect);
        let items = menu.labels().map(ListItem::new);
        let mut state = ListState::default().with_selected(Some(menu.selected));
        let menu_bg = if theme == Theme::Glass {
            Color::Rgb(15, 25, 28)
        } else {
            palette.surface_alt
        };
        frame.render_stateful_widget(
            List::new(items)
                .block(block_themed(" ACTIONS  ·  Esc close ", true, theme))
                .style(Style::default().fg(palette.text).bg(menu_bg))
                .highlight_style(selected_row_style(theme))
                .highlight_symbol(selected_marker_text(theme)),
            rect,
            &mut state,
        );
        render.mouse_hits.clear();
        for index in 0..menu.labels().len().min(height.saturating_sub(2) as usize) {
            hit(
                render,
                Rect::new(
                    rect.x + 1,
                    rect.y + 1 + index as u16,
                    width.saturating_sub(2),
                    1,
                ),
                MouseTarget::Menu(index),
            );
        }
    }
}
