mod chrome;
use chrome::{context_menu, footer, header};
pub(crate) mod background;
pub(crate) use background::BackgroundState;
mod help;
use help::help;
mod terminal;
pub use terminal::{TerminalGuard, set_title};
mod theme;
pub use theme::Theme;
mod widgets;
use widgets::*;
mod navigation;
use navigation::*;
mod visualizer;
use visualizer::*;
mod lyrics;
use lyrics::*;
mod stats;
use stats::*;
mod catalog;
#[allow(unused_imports)]
pub use catalog::format_breadcrumb_trail;
use catalog::*;
mod queue;
use queue::*;
mod playback;
use playback::*;
mod mix_builder;
use mix_builder::*;

use crate::{
    app::{App, MouseTarget, Overlay, RenderState, SearchScope, State, View},
    catalog::Rows,
    model::Repeat,
};
use anyhow::Result;
use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, SetTitle, disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{
    prelude::*,
    widgets::{
        Block, BorderType, Borders, Cell, Clear, Gauge, List, ListItem, ListState, Paragraph, Row,
        Table, TableState, Wrap,
    },
};
use std::io::{Stdout, stdout};

/// Native wallpaper windows can retain reflowed text after a resize or view change.
/// Repaint their complete screen so terminal-side contents cannot outlive our buffer.
pub fn draw_terminal<B: Backend>(terminal: &mut Terminal<B>, app: &App) -> std::io::Result<()> {
    if app.config.native_glass {
        terminal.clear()?;
    }
    terminal.draw(|frame| draw(frame, app, &mut app.ui.render.borrow_mut()))?;
    Ok(())
}

pub fn draw(frame: &mut Frame<'_>, app: &App, render: &mut RenderState) {
    let area = frame.area();
    render.mouse_hits.clear();
    render.terminal_size = (area.width, area.height);
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    frame.render_widget(
        Block::default().style(Style::default().fg(palette.text).bg(palette.background)),
        area,
    );
    if area.width < 32 || area.height < 10 {
        frame.render_widget(
            Paragraph::new(format!(
                "TUITIFY v{}\nResize to 32x10 or larger.\nq quit | Space pause",
                env!("CARGO_PKG_VERSION")
            ))
            .style(Style::default().fg(theme.primary())),
            area,
        );
        background::apply(frame, app, render, theme);
        return;
    }
    let compact = area.height < 18;
    if app.ui.overlay == Overlay::MixBuilder && area.height < 12 {
        let vertical = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(area);
        header(frame, app, vertical[0]);
        body(frame, app, render, vertical[1]);
        footer(frame, app, vertical[2]);
        background::apply(frame, app, render, theme);
        return;
    }
    let vertical = Layout::vertical([
        Constraint::Length(if compact { 1 } else { 2 }),
        Constraint::Min(3),
        Constraint::Length(if area.height >= 24 { 5 } else { 4 }),
        Constraint::Length(if compact { 1 } else { 3 }),
    ])
    .split(area);

    header(frame, app, vertical[0]);

    body(frame, app, render, vertical[1]);
    playback(frame, app, render, vertical[2]);

    footer(frame, app, vertical[3]);
    context_menu(frame, app, render, area);
    background::apply(frame, app, render, theme);
}

fn center(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    hit(render, area, MouseTarget::CatalogScroll);
    match app.ui.overlay {
        Overlay::Visualizer => visualizer(frame, app, area),
        Overlay::Lyrics => lyrics(frame, app, render, area),
        Overlay::Stats => stats(frame, app, render, area),
        Overlay::MixBuilder => mix_builder(frame, app, render, area),
        Overlay::None => match app.catalog.view {
            View::Help => help(frame, app, render, area),
            View::Queue => queue(frame, app, render, area, true),
            View::Search | View::Playlists | View::Liked | View::Album | View::Artist => {
                catalog(frame, app, render, area)
            }
        },
    }
}

fn hit(render: &mut RenderState, area: Rect, target: MouseTarget) {
    if area.width > 0 && area.height > 0 {
        render.mouse_hits.push((area, target));
    }
}
fn row_hits(
    render: &mut RenderState,
    area: Rect,
    visible: &std::ops::Range<usize>,
    queue: bool,
    header: bool,
) {
    let top = area.y + if header { 3 } else { 1 };
    for (line, index) in visible.clone().enumerate() {
        let y = top + line as u16;
        if y >= area.bottom().saturating_sub(1) {
            break;
        }
        hit(
            render,
            Rect::new(area.x + 1, y, area.width.saturating_sub(2), 1),
            if queue {
                MouseTarget::Queue(index)
            } else {
                MouseTarget::Catalog(index)
            },
        );
    }
}

fn viewport(
    selected: usize,
    len: usize,
    height: usize,
    offset: &mut usize,
) -> std::ops::Range<usize> {
    let height = height.max(1);
    let selected = selected.min(len.saturating_sub(1));
    let mut start = (*offset).min(len.saturating_sub(height));
    if selected < start {
        start = selected;
    }
    if selected >= start + height {
        start = selected + 1 - height;
    }
    *offset = start;
    start..(start + height).min(len)
}

#[cfg(test)]
mod tests;
