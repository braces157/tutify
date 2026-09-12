use super::*;

pub(super) fn help(frame: &mut Frame<'_>, app: &App, render: &mut RenderState, area: Rect) {
    let theme = Theme::from_str(&app.config.theme);
    let palette = theme.palette();
    let text = "Tuitify, your way\n\n\
    Navigation\n\
    1-5 / Tab      Switch views / toggle sidebar focus\n\
    Up/Down, j/k   Move cursor in current view\n\
    Enter          Play track or open playlist\n\
    Backspace      Return from playlist to playlists index\n\
    Esc            Close Help / exit Lyrics, Visualizer, or Stats\n\n\
    Mouse controls\n\
    Left click     Select row / switch view / edit search\n\
    Right click    Track or playlist actions (Esc closes)\n\
    Wheel          Move three rows; scroll Help/plain lyrics\n\
    Badge / bar    Play-pause / seek\n\
    Ctrl+Shift+V   Intentional terminal paste into search/filter\n\n\
    Playback\n\
    Media keys     Play/pause, next/previous outside terminal\n\
    Space          Play, pause, or retry failed playback\n\
    n / p          Next / previous track (restarts after 3s)\n\
    Left / Right   Seek backward / forward 10 seconds\n\
    Home / End     Jump to beginning / end of track\n\n\
    Volume\n\
    + / -          Volume up / down 5%\n\
    [ / ]          Fine volume control 1%\n\
    m              Mute / restore previous volume\n\n\
    Queue & playlists\n\
    u / Ctrl-Z     Undo queue edit (restores paused)\n\
    a              Append selected track to Queue (or enqueue entire playlist)\n\
    A (Shift-A)    Play Next (insert directly after current track)\n\
    R (Shift-R)    Start Track Radio (play track & queue related recommendations)\n\
    K / J          Move selected track Up / Down in Queue\n\
    d / x / Delete Remove selected item from Queue\n\
    C (Shift-C)    Clear entire Queue\n\
    . or c         Jump to currently playing track in Queue\n\
    s              Cycle shuffle: off / on / Smart (✦ suggestions)\n\
    r              Cycle repeat: Off -> Queue -> Track\n\n\
    Views & themes\n\
    t              Cycle Retro Theme (Spotify, Amber CRT, Matrix, Cyberpunk, Monochrome)\n\
    v              Toggle Retro Visualizer (Real-time FFT)\n\
    l              Toggle Synced Real-Time Lyrics View (Lrclib)\n\
    S              Toggle local aggregate song statistics overlay\n\n\
    M (Shift-M)    Open Mix Builder (queue or current Spotify playlist)\n\
    Catalog & network\n\
    / or f         Filter loaded Liked/Playlist rows only\n\
    F2             Search Spotify catalog\n\
    F3             Search all saved Liked Songs/playlist tracks\n\
    Esc            Cancel library scan; retain partial matches\n\
    PgDn           Load next catalog page\n\
    F5             Refresh / retry network connection\n\
    q / Ctrl-C     Quit and save state\n\n\
    Troubleshooting\n\
    Login issue? Exit and run `tuitify auth --force`.\n\
    Streaming issue? Run `tuitify auth --streaming --force`.\n\
    No audio? Check Windows default output device and Spotify Premium.";
    let help_title = format!(" HELP  ·  v{} ", env!("CARGO_PKG_VERSION"));
    let outer = block_themed(help_title, !app.catalog.sidebar, theme);
    let inner = outer.inner(area);
    let mut lines = Vec::new();
    for raw in text.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            lines.push(Line::default());
        } else if !raw.starts_with("    ") || !trimmed.contains("  ") {
            let style = if trimmed.ends_with('.') || trimmed.contains("issue?") {
                Style::default().fg(palette.text_muted)
            } else {
                Style::default().fg(palette.primary_soft).bold()
            };
            lines.push(Line::from(Span::styled(trimmed.to_owned(), style)));
        } else if let Some((key, description)) = trimmed.split_once("  ") {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<15}", key.trim()),
                    Style::default().fg(palette.primary).bold(),
                ),
                Span::styled(
                    description.trim().to_owned(),
                    Style::default().fg(palette.text_muted),
                ),
            ]));
        }
    }
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let max_scroll = paragraph
        .line_count(inner.width)
        .saturating_sub(inner.height as usize);
    render.help_length = max_scroll + 1;
    frame.render_widget(outer, area);
    frame.render_widget(
        paragraph.scroll((
            app.catalog.selected.min(max_scroll).min(u16::MAX as usize) as u16,
            0,
        )),
        inner,
    );
}
