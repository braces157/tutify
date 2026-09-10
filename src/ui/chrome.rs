use super::*;

pub(super) fn header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let version_str = if app.demo {
        format!("v{} • DEMO • SIMULATED", env!("CARGO_PKG_VERSION"))
    } else {
        format!("v{}", env!("CARGO_PKG_VERSION"))
    };
    let header_line = if area.width >= 86 {
        Line::from(vec![
            Span::styled(
                " TUITIFY ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.primary())
                    .bold(),
            ),
            Span::styled(
                format!(" {version_str} "),
                Style::default().fg(theme.primary()).bold(),
            ),
            Span::styled(
                format!("[{}]", theme.name()),
                Style::default().fg(theme.accent_dim()).bold(),
            ),
            Span::styled("  YOUR MUSIC, IN THE TERMINAL", Style::default().fg(MUTED)),
            Span::styled(
                "   [? help]  [q quit]  [t theme]",
                Style::default().fg(theme.accent_dim()),
            ),
        ])
    } else if area.width >= 58 {
        Line::from(vec![
            Span::styled(
                " TUITIFY ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.primary())
                    .bold(),
            ),
            Span::styled(
                format!(" {version_str} "),
                Style::default().fg(theme.primary()).bold(),
            ),
            Span::styled(
                format!("[{}]", theme.name()),
                Style::default().fg(theme.accent_dim()).bold(),
            ),
            Span::styled(
                "   [? help]  [q quit]  [t theme]",
                Style::default().fg(theme.accent_dim()),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                format!(" TUITIFY {version_str}"),
                Style::default().fg(theme.primary()).bold(),
            ),
            Span::styled("  ? help", Style::default().fg(MUTED)),
        ])
    };
    frame.render_widget(Paragraph::new(header_line), area);
}

pub(super) fn footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let status_split = Layout::vertical([Constraint::Length(1), Constraint::Length(2)]).split(area);

    let auth_expired = app.catalog_health == crate::catalog::Health::AuthenticationRequired;

    let status_line = if auth_expired {
        Line::from(vec![
            Span::styled(
                " ! AUTH EXPIRED ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(Color::Yellow)
                    .bold(),
            ),
            Span::styled(
                "  Spotify login expired. Exit (q) and run 'tuitify auth --force' in terminal to reconnect.",
                Style::default().fg(Color::Yellow).bold(),
            ),
        ])
    } else if app.state == State::Failed {
        Line::from(vec![
            Span::styled(" ! ", Style::default().fg(Color::LightRed).bold()),
            Span::styled(&app.status, Style::default().fg(Color::LightRed)),
        ])
    } else if app.catalog.busy {
        Line::from(vec![
            Span::styled(" ... ", Style::default().fg(Color::Yellow).bold()),
            Span::styled(&app.status, Style::default().fg(Color::Yellow)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" * ", Style::default().fg(theme.primary())),
            Span::styled(&app.status, Style::default().fg(MUTED)),
        ])
    };
    frame.render_widget(
        Paragraph::new(status_line).wrap(Wrap { trim: true }),
        status_split[0],
    );

    let shortcuts = if app.ui.overlay == Overlay::MixBuilder {
        Line::from(if area.width >= 60 {
            " Enter Replace  A Append  p Pin  g Regenerate  ? Details  Esc Cancel "
        } else {
            " Enter/A Apply  p Pin  g Regen  ? More  Esc Cancel "
        })
        .style(Style::default().fg(theme.accent_dim()).bold())
    } else if area.width >= 80 {
        Line::from(vec![
            Span::styled(
                " Space ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Play/Pause  ", Style::default().fg(MUTED)),
            Span::styled(
                " / ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(
                if matches!(app.catalog.view, View::Liked | View::Playlists) {
                    " Filter  "
                } else {
                    " Search  "
                },
                Style::default().fg(MUTED),
            ),
            Span::styled(
                " 1-5 ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Views  ", Style::default().fg(MUTED)),
            Span::styled(
                " Tab ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Focus  ", Style::default().fg(MUTED)),
            Span::styled(
                " n/p ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Next/Prev  ", Style::default().fg(MUTED)),
            Span::styled(
                " +/- ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Vol  ", Style::default().fg(MUTED)),
            Span::styled(
                " ? ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Help  ", Style::default().fg(MUTED)),
            Span::styled(
                " q ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Quit", Style::default().fg(MUTED)),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                " Space ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Play  ", Style::default().fg(MUTED)),
            Span::styled(
                " / ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(
                if matches!(app.catalog.view, View::Liked | View::Playlists) {
                    " Filter  "
                } else {
                    " Search  "
                },
                Style::default().fg(MUTED),
            ),
            Span::styled(
                " Tab ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Focus  ", Style::default().fg(MUTED)),
            Span::styled(
                " q ",
                Style::default()
                    .fg(Color::Rgb(14, 17, 16))
                    .bg(theme.accent_dim())
                    .bold(),
            ),
            Span::styled(" Quit", Style::default().fg(MUTED)),
        ])
    };
    frame.render_widget(Paragraph::new(shortcuts), status_split[1]);
}

pub(super) fn context_menu(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
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
        frame.render_stateful_widget(
            List::new(items)
                .block(block_themed(" Actions • Esc close ", true, theme))
                .style(Style::default().fg(FG).bg(BG))
                .highlight_style(
                    Style::default()
                        .fg(theme.primary())
                        .bg(theme.highlight_bg())
                        .bold(),
                ),
            rect,
            &mut state,
        );
        // While a menu is open, clicks cannot activate the covered controls.
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
