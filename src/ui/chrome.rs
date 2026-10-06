use super::*;

pub(super) fn header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let version = if app.demo {
        format!("v{} / demo", env!("CARGO_PKG_VERSION"))
    } else if app.config.source == crate::model::MusicSource::Youtube && app.config.youtube_music {
        format!("v{} / YouTube Music", env!("CARGO_PKG_VERSION"))
    } else if app.config.source == crate::model::MusicSource::Youtube {
        format!("v{} / YouTube", env!("CARGO_PKG_VERSION"))
    } else {
        format!("v{}", env!("CARGO_PKG_VERSION"))
    };

    frame.render_widget(
        Block::default().style(Style::default().bg(palette.surface_alt)),
        area,
    );

    if area.width >= 58 {
        let command_width = if area.width >= 86 { 38 } else { 24 };
        let parts = Layout::horizontal([Constraint::Min(24), Constraint::Length(command_width)])
            .split(area);
        let identity = Line::from(vec![
            Span::styled(" TUITIFY", Style::default().fg(palette.primary).bold()),
            Span::styled(
                format!("  {version}"),
                Style::default().fg(palette.text_muted),
            ),
            Span::styled(
                format!(" / Theme: {}", theme.name()),
                Style::default().fg(palette.text_subtle),
            ),
        ]);
        let commands = if area.width >= 86 {
            "F6 Tools   ? Help   t Theme   q Quit "
        } else {
            "F6 Tools ? Help q Quit "
        };
        frame.render_widget(Paragraph::new(identity), parts[0]);
        frame.render_widget(
            Paragraph::new(commands)
                .style(Style::default().fg(palette.text_muted))
                .alignment(Alignment::Right),
            parts[1],
        );
    } else {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" TUITIFY", Style::default().fg(palette.primary).bold()),
                Span::styled(
                    format!("  {version}"),
                    Style::default().fg(palette.text_muted),
                ),
                Span::styled("   F6 Tools", Style::default().fg(palette.text_muted)),
            ])),
            area,
        );
    }
}

pub(super) fn footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
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
            Paragraph::new(format!("{message}{filename}"))
                .style(Style::default().fg(palette.text_muted))
                .wrap(Wrap { trim: true }),
            area,
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
                Style::default().fg(palette.status_warning).bold(),
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
            Span::styled(" … LOADING ", warning_badge(theme)),
            Span::styled(
                format!("  {}", app.status),
                Style::default().fg(palette.status_warning),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ● ", Style::default().fg(palette.primary_soft)),
            Span::styled(&app.status, Style::default().fg(palette.text_muted)),
        ])
    };
    frame.render_widget(
        Paragraph::new(status_line).wrap(Wrap { trim: true }),
        status_split[0],
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
        if area.width >= 80 {
            push_hint(&mut hints, "1–5", "Views", false, theme);
        }
        if area.width >= 100 {
            push_hint(&mut hints, "n/p", "Next/Prev", false, theme);
            push_hint(&mut hints, "+/−", "Volume", false, theme);
        }
    }
    frame.render_widget(Paragraph::new(Line::from(hints)), status_split[1]);
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
        format!(" {label}  "),
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
                .highlight_symbol("▌ "),
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
