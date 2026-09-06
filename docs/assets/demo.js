(function () {
  'use strict';

  const catalog = [
    {
      id: '1', title: 'Bohemian Rhapsody', artist: 'Queen', duration: '5:54', sec: 354,
      lyrics: [
        { time: 10, text: 'Is this the real life? Is this just fantasy?' },
        { time: 25, text: 'Caught in a landslide, no escape from reality' },
        { time: 40, text: 'Open your eyes, look up to the skies and see' },
        { time: 55, text: "I'm just a poor boy, I need no sympathy" },
        { time: 70, text: 'Because I\'m easy come, easy go, little high, little low' },
        { time: 85, text: "Any way the wind blows doesn't really matter to me, to me" },
        { time: 100, text: 'Mama, just killed a man' },
        { time: 115, text: "Put a gun against his head, pulled my trigger, now he's dead" },
        { time: 130, text: 'Mama, life had just begun' },
        { time: 145, text: "But now I've gone and thrown it all away" },
        { time: 160, text: 'Too late, my time has come' },
        { time: 175, text: "Sends shivers down my spine, body's aching all the time" },
        { time: 190, text: 'Goodbye, everybody, I\'ve got to go' },
        { time: 205, text: 'Gotta leave you all behind and face the truth' },
      ],
    },
    {
      id: '2', title: 'Yellow', artist: 'Coldplay', duration: '4:29', sec: 269,
      lyrics: [
        { time: 8, text: 'Look at the stars, look how they shine for you' },
        { time: 24, text: 'And everything you do' },
        { time: 40, text: 'Yeah, they were all yellow' },
        { time: 57, text: 'I came along, I wrote a song for you' },
        { time: 74, text: 'And all the things you do' },
      ],
    },
    {
      id: '3', title: 'Under Pressure', artist: 'Queen, David Bowie', duration: '4:08', sec: 248,
      lyrics: [
        { time: 12, text: 'Pressure pushing down on me' },
        { time: 30, text: 'Pressing down on you, no man asks for' },
        { time: 49, text: 'Under pressure that brings a building down' },
        { time: 68, text: "It's the terror of knowing what this world is about" },
      ],
    },
    { id: '4', title: 'Get Lucky', artist: 'Daft Punk, Pharrell Williams', duration: '6:09', sec: 369 },
    { id: '5', title: 'Creep', artist: 'Radiohead', duration: '3:58', sec: 238 },
    { id: '6', title: 'Comfortably Numb', artist: 'Pink Floyd', duration: '6:22', sec: 382 },
    { id: '7', title: 'Starboy', artist: 'The Weeknd, Daft Punk', duration: '3:50', sec: 230 },
    { id: '8', title: 'Viva La Vida', artist: 'Coldplay', duration: '4:02', sec: 242 },
  ];

  const radioPool = [
    { id: 'r1', title: 'Somebody to Love', artist: 'Queen', duration: '4:56', sec: 296 },
    { id: 'r2', title: "Don't Stop Me Now", artist: 'Queen', duration: '3:29', sec: 209 },
    { id: 'r3', title: 'The Scientist', artist: 'Coldplay', duration: '5:09', sec: 309 },
    { id: 'r4', title: 'Heroes', artist: 'David Bowie', duration: '6:11', sec: 371 },
    { id: 'r5', title: 'Karma Police', artist: 'Radiohead', duration: '4:21', sec: 261 },
    { id: 'r6', title: 'Instant Crush', artist: 'Daft Punk', duration: '5:37', sec: 337 },
  ];

  const themes = [
    { id: 'theme-spotify', name: 'Classic Spotify' },
    { id: 'theme-phosphor', name: 'Phosphor Green CRT' },
    { id: 'theme-amber', name: 'Amber VT220 CRT' },
    { id: 'theme-monochrome', name: 'Monochrome Silver' },
    { id: 'theme-cyberpunk', name: 'Cyberpunk Neon' },
  ];

  const playlistTrackIds = ['1', '3', '6', '8'];
  const likedTrackIds = ['1', '2', '3', '5', '8'];

  let queue = catalog.slice();
  let currentThemeIdx = 0;
  let showVisualizer = false;
  let showLyrics = false;
  let currentTrack = catalog[0];
  let isPlaying = true;
  let currentSec = 74;
  let selectedIndex = 0;
  let activeFilter = '';
  let currentView = 3;
  let shortcutsEnabled = true;
  let demoVisible = true;
  let pageVisible = !document.hidden;
  let demoTimerId = null;
  let toastTimerId = null;
  let lastLyricsRenderKey = '';
  let playbackBaseSec = currentSec;
  let playbackAnchorMs = performance.now();

  const get = (id) => document.getElementById(id);

  function escapeHtml(value) {
    return String(value)
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;')
      .replaceAll('"', '&quot;')
      .replaceAll("'", '&#39;');
  }

  function formatTime(seconds) {
    const safeSeconds = Math.max(0, Math.floor(seconds));
    const minutes = Math.floor(safeSeconds / 60);
    const remainder = safeSeconds % 60;
    return `${minutes}:${String(remainder).padStart(2, '0')}`;
  }

  function trackById(id) {
    return catalog.find((track) => track.id === id) || queue.find((track) => track.id === id);
  }

  function tracksForView() {
    if (currentView === 1) return catalog;
    if (currentView === 2) return playlistTrackIds.map(trackById).filter(Boolean);
    if (currentView === 3) return likedTrackIds.map(trackById).filter(Boolean);
    if (currentView === 4) return queue;
    return [];
  }

  function filteredTracks() {
    const terms = activeFilter.toLowerCase().trim().split(/\s+/).filter(Boolean);
    if (!terms.length) return tracksForView();
    return tracksForView().filter((track) => {
      const title = track.title.toLowerCase();
      const artist = track.artist.toLowerCase();
      return terms.every((term) => title.includes(term) || artist.includes(term));
    });
  }

  function viewMeta() {
    return {
      1: { title: 'SEARCH RESULTS', description: 'Sample catalog • filter by title or artist' },
      2: { title: 'PLAYLISTS • LATE NIGHT MIX', description: 'A small playlist sample' },
      3: { title: 'LIKED SONGS', description: 'Saved sample tracks' },
      4: { title: 'QUEUE', description: 'Tracks waiting to play' },
      5: { title: 'HELP', description: 'Keyboard and control reference' },
    }[currentView];
  }

  function applyDemoTheme() {
    const screen = get('tui-terminal-screen');
    const theme = themes[currentThemeIdx];
    themes.forEach((item) => screen.classList.remove(item.id));
    screen.classList.add(theme.id);
    get('tui-theme-label').innerText = theme.name;
    renderRows();
  }

  function cycleDemoTheme() {
    currentThemeIdx = (currentThemeIdx + 1) % themes.length;
    applyDemoTheme();
  }

  function updateToggleButtons() {
    const visualizerButton = get('tui-btn-vis');
    const lyricsButton = get('tui-btn-lyrics');
    visualizerButton.setAttribute('aria-pressed', String(showVisualizer));
    lyricsButton.setAttribute('aria-pressed', String(showLyrics));
  }

  function toggleDemoVisualizer() {
    showVisualizer = !showVisualizer;
    if (showVisualizer) showLyrics = false;
    updatePanelViews();
  }

  function toggleDemoLyrics() {
    showLyrics = !showLyrics;
    if (showLyrics) showVisualizer = false;
    lastLyricsRenderKey = '';
    updatePanelViews();
    updateLyricsDisplay(true);
  }

  function updatePanelViews() {
    const catalogView = get('tui-catalog-view');
    const lyricsView = get('tui-lyrics-view');
    const visualizerView = get('tui-visualizer-view');
    const helpView = get('tui-help-view');
    const filterBox = get('tui-filter-box');
    const overlayVisible = showLyrics || showVisualizer;
    const helpVisible = currentView === 5 && !overlayVisible;

    catalogView.classList.toggle('hidden', overlayVisible || helpVisible);
    lyricsView.classList.toggle('hidden', !showLyrics);
    visualizerView.classList.toggle('hidden', !showVisualizer);
    helpView.classList.toggle('hidden', !helpVisible);
    filterBox.classList.toggle('hidden', currentView === 5 || overlayVisible);
    lyricsView.setAttribute('aria-hidden', String(!showLyrics));
    visualizerView.setAttribute('aria-hidden', String(!showVisualizer));
    helpView.setAttribute('aria-hidden', String(!helpVisible));
    updateToggleButtons();
    updateLyricsDisplay();
  }

  function triggerDemoRadio() {
    const additions = radioPool.filter((radioTrack) => !catalog.some((track) => track.id === radioTrack.id));
    catalog.push(...additions);
    queue.push(...additions);
    get('tui-queue-count').innerText = String(queue.length);

    const toast = get('tui-toast');
    toast.innerText = additions.length
      ? `✨ Track Radio: queued ${additions.length} related tracks.`
      : '✨ Track Radio: no new tracks to add.';
    toast.classList.remove('hidden');
    if (toastTimerId !== null) window.clearTimeout(toastTimerId);
    toastTimerId = window.setTimeout(() => toast.classList.add('hidden'), 3500);
    renderRows();
  }

  function activeLyricsIndex(lyrics) {
    let index = -1;
    lyrics.forEach((line, lineIndex) => {
      if (currentSec >= line.time) index = lineIndex;
    });
    return index;
  }

  function updateLyricsDisplay(force = false) {
    if (!showLyrics) {
      lastLyricsRenderKey = '';
      return;
    }

    const container = get('tui-lyrics-content');
    const heading = get('tui-lyrics-heading');
    if (!container || !heading) return;
    const lyrics = Array.isArray(currentTrack.lyrics) ? currentTrack.lyrics : [];
    const currentIndex = activeLyricsIndex(lyrics);
    const renderKey = `${currentTrack.id}:${currentIndex}:${lyrics.length}`;
    if (!force && renderKey === lastLyricsRenderKey) return;
    lastLyricsRenderKey = renderKey;
    heading.innerText = `🎤 Sample Lyrics — ${currentTrack.title}`;

    if (!lyrics.length) {
      container.innerHTML = `<p class="text-slate-500 text-xs sm:text-sm">No sample lyrics are available for ${escapeHtml(currentTrack.title)}.</p>`;
      return;
    }

    const start = Math.max(0, currentIndex - 2);
    const end = Math.min(lyrics.length, Math.max(currentIndex + 3, 3));
    const visible = lyrics.slice(start, end);
    container.innerHTML = visible.map((line, offset) => {
      const lineIndex = start + offset;
      const text = escapeHtml(line.text);
      if (lineIndex === currentIndex) {
        return `<div class="text-[var(--term-accent)] font-bold text-sm sm:text-base py-1 px-3 bg-white/[0.05] rounded border border-[var(--term-accent)]/30 animate-pulse">${text}</div>`;
      }
      return `<div class="text-slate-500 text-xs sm:text-sm transition-opacity">${text}</div>`;
    }).join('');
  }

  function updateViewMeta() {
    const meta = viewMeta();
    const matches = filteredTracks();
    get('tui-table-title').firstChild.textContent = `${meta.title} • `;
    get('tui-match-count').innerText = `${matches.length} of ${tracksForView().length} matches`;
    get('tui-view-description').innerText = meta.description;
    get('tui-catalog-view').setAttribute('aria-labelledby', `tui-tab-${currentView}`);
  }

  function renderRows() {
    const container = get('tui-rows-container');
    if (!container) return;
    const focusedId = container.contains(document.activeElement) ? document.activeElement.dataset.trackId : null;
    updateViewMeta();
    if (currentView === 5) {
      container.innerHTML = '';
      return;
    }

    const matches = filteredTracks();
    if (selectedIndex >= matches.length) selectedIndex = Math.max(0, matches.length - 1);
    if (!matches.length) {
      container.innerHTML = `
        <div class="py-8 text-center text-slate-500 text-xs">
          No tracks match your filter.<br>
          <span class="text-slate-600 text-[11px]">Press Backspace to edit or Esc to clear.</span>
        </div>`;
      return;
    }

    container.innerHTML = matches.map((track, index) => {
      const isCurrent = track.id === currentTrack.id;
      const isSelected = index === selectedIndex;
      const indicator = isCurrent ? (isPlaying ? '►' : '||') : '  ';
      const label = `Play ${track.title} by ${track.artist}`;
      return `
        <button type="button" class="tui-track-row w-full px-3 py-2 flex items-center justify-between text-left cursor-pointer transition ${isSelected ? 'tui-track-row-selected' : ''}" data-track-id="${escapeHtml(track.id)}" aria-label="${escapeHtml(label)}"${isCurrent ? ' aria-current="true"' : ''}>
          <span class="flex items-center gap-3 truncate">
            <span class="w-6 text-center ${isCurrent ? 'text-[var(--term-accent)] font-bold' : 'text-slate-600'}" aria-hidden="true">${indicator}</span>
            <span class="text-slate-500 w-6" aria-hidden="true">${index + 1}</span>
            <span class="${isCurrent ? 'text-[var(--term-accent)] font-bold' : 'text-slate-200'} truncate">${escapeHtml(track.title)}</span>
            <span class="text-slate-400 text-xs truncate hidden sm:inline">— ${escapeHtml(track.artist)}</span>
          </span>
          <span class="text-slate-500 text-xs shrink-0 ml-4">${escapeHtml(track.duration)}</span>
        </button>`;
    }).join('');
    if (focusedId) {
      const replacement = Array.from(container.querySelectorAll('[data-track-id]')).find((row) => row.dataset.trackId === focusedId);
      if (replacement) replacement.focus({ preventScroll: true });
    }
  }

  function selectAndPlay(id) {
    const found = trackById(id);
    if (!found) return;
    currentTrack = found;
    currentSec = 0;
    isPlaying = true;
    playbackBaseSec = currentSec;
    playbackAnchorMs = performance.now();
    const matches = filteredTracks();
    selectedIndex = Math.max(0, matches.findIndex((track) => track.id === id));
    lastLyricsRenderKey = '';
    updatePlayerUI();
    renderRows();
    refreshDemoTimer();
  }

  function togglePlay() {
    if (isPlaying) currentSec = elapsedPlaybackSeconds();
    isPlaying = !isPlaying;
    playbackBaseSec = currentSec;
    playbackAnchorMs = performance.now();
    updatePlayerUI();
    renderRows();
    refreshDemoTimer();
  }

  function updatePlayerUI() {
    const badge = get('tui-now-playing-badge');
    const button = get('tui-play-btn');
    const headerTrack = get('tui-header-track');
    const screen = get('tui-terminal-screen');
    headerTrack.innerText = `${currentTrack.title} — ${currentTrack.artist}`;

    badge.innerText = isPlaying ? '► PLAYING' : '|| PAUSED';
    badge.className = isPlaying
      ? 'px-2 py-0.5 rounded bg-[var(--term-accent)] text-black font-bold text-xs'
      : 'px-2 py-0.5 rounded bg-white/10 text-slate-300 font-bold text-xs border border-white/20';
    button.innerText = isPlaying ? '⏸' : '▶';
    button.setAttribute('aria-label', isPlaying ? 'Pause demo playback' : 'Play demo playback');
    button.setAttribute('aria-pressed', String(isPlaying));
    screen.classList.toggle('is-playing', isPlaying);

    get('tui-curr-time').innerText = formatTime(currentSec);
    get('tui-total-time').innerText = currentTrack.duration;
    const seek = get('tui-seek');
    seek.max = String(currentTrack.sec);
    seek.value = String(Math.min(currentTrack.sec, currentSec));
    seek.setAttribute('aria-valuetext', `${formatTime(currentSec)} of ${currentTrack.duration}`);
    updateLyricsDisplay();
  }

  function handleFilterInput(value) {
    activeFilter = value;
    selectedIndex = 0;
    renderRows();
  }

  function handleFilterKeydown(event) {
    if (event.key === 'Enter') {
      const matches = filteredTracks();
      if (matches[0]) selectAndPlay(matches[0].id);
      event.preventDefault();
    } else if (event.key === 'Escape') {
      clearFilter(event);
    }
  }

  function clearFilter(event) {
    if (event) event.preventDefault();
    activeFilter = '';
    const input = get('tui-filter-input');
    input.value = '';
    selectedIndex = 0;
    renderRows();
  }

  function switchView(viewIndex) {
    currentView = Number(viewIndex);
    showVisualizer = false;
    showLyrics = false;
    const matches = filteredTracks();
    const currentIndex = matches.findIndex((track) => track.id === currentTrack.id);
    selectedIndex = currentIndex >= 0 ? currentIndex : 0;
    document.querySelectorAll('.tui-tab').forEach((tab, index) => {
      const active = index + 1 === currentView;
      tab.setAttribute('aria-selected', String(active));
      tab.tabIndex = active ? 0 : -1;
      tab.classList.toggle('tui-tab-active', active);
    });
    updatePanelViews();
    renderRows();
  }

  function handleVolumeChange(value) {
    const volume = Math.max(0, Math.min(100, Number(value) || 0));
    get('tui-vol-label').innerText = `VOL ${volume}%`;
    const input = get('tui-volume');
    input.value = String(volume);
    input.setAttribute('aria-valuetext', `${volume} percent`);
  }

  function handleSeekInput(value) {
    currentSec = Math.max(0, Math.min(currentTrack.sec, Number(value) || 0));
    playbackBaseSec = currentSec;
    playbackAnchorMs = performance.now();
    updatePlayerUI();
    refreshDemoTimer();
  }

  function seekAudio(event) {
    if (event && event.target && event.target.value !== undefined) {
      handleSeekInput(event.target.value);
    }
  }

  function switchInstallTab(tabIndex) {
    for (let index = 0; index < 3; index += 1) {
      const button = get(`install-tab-${index}`);
      const content = get(`install-content-${index}`);
      const active = index === Number(tabIndex);
      button.setAttribute('aria-selected', String(active));
      button.tabIndex = active ? 0 : -1;
      button.className = active
        ? 'px-5 py-3 text-spotify font-semibold border-b-2 border-spotify bg-white/[0.02]'
        : 'px-5 py-3 text-slate-400 hover:text-white transition';
      content.classList.toggle('hidden', !active);
    }
  }

  function copyCommand(text, button) {
    const originalHtml = button.innerHTML;
    const originalLabel = button.getAttribute('aria-label');
    const restore = () => {
      button.innerHTML = originalHtml;
      button.setAttribute('aria-label', originalLabel || 'Copy command');
    };
    const showResult = (success) => {
      button.innerHTML = success
        ? '<span aria-hidden="true">✓</span> Copied'
        : '<span aria-hidden="true">⚠</span> Select command above to copy';
      button.setAttribute('aria-label', success ? 'Command copied' : 'Copy unavailable; select the command text above');
      window.setTimeout(restore, 3500);
    };

    if (!navigator.clipboard || typeof navigator.clipboard.writeText !== 'function') {
      showResult(false);
      return;
    }
    Promise.resolve()
      .then(() => navigator.clipboard.writeText(text))
      .then(() => showResult(true))
      .catch(() => showResult(false));
  }

  function elapsedPlaybackSeconds() {
    return Math.max(0, playbackBaseSec + Math.floor((performance.now() - playbackAnchorMs) / 1000));
  }

  function setCurrentTrackByDirection(direction) {
    const source = tracksForView().length ? tracksForView() : queue;
    if (!source.length) return;
    const index = source.findIndex((track) => track.id === currentTrack.id);
    const nextIndex = (index + direction + source.length) % source.length;
    currentTrack = source[nextIndex];
    currentSec = 0;
    playbackBaseSec = currentSec;
    playbackAnchorMs = performance.now();
    const matches = filteredTracks();
    const selected = matches.findIndex((track) => track.id === currentTrack.id);
    selectedIndex = selected >= 0 ? selected : 0;
    lastLyricsRenderKey = '';
    updatePlayerUI();
    renderRows();
    refreshDemoTimer();
  }

  function updateClock() {
    const now = new Date();
    const clock = get('tui-clock');
    if (clock) clock.innerText = `${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}:${String(now.getSeconds()).padStart(2, '0')}`;
  }

  function canRunDemoTimer() {
    return pageVisible && demoVisible && isPlaying;
  }

  function stopDemoTimer() {
    if (demoTimerId !== null) {
      window.clearTimeout(demoTimerId);
      demoTimerId = null;
    }
  }

  function runDemoTick() {
    demoTimerId = null;
    if (!canRunDemoTimer()) return;
    const elapsed = elapsedPlaybackSeconds();
    if (elapsed >= currentTrack.sec) {
      setCurrentTrackByDirection(1);
    } else if (elapsed !== currentSec) {
      currentSec = elapsed;
      updatePlayerUI();
    }
    updateClock();
    refreshDemoTimer();
  }

  function refreshDemoTimer() {
    stopDemoTimer();
    if (canRunDemoTimer()) demoTimerId = window.setTimeout(runDemoTick, 1000);
  }

  function setDemoVisibility(visible) {
    demoVisible = visible;
    const screen = get('tui-terminal-screen');
    screen.classList.toggle('demo-offscreen', !visible);
    refreshDemoTimer();
    if (canRunDemoTimer()) runDemoTick();
  }

  function setPageVisibility(visible) {
    pageVisible = visible;
    get('tui-terminal-screen').classList.toggle('demo-hidden', !visible);
    refreshDemoTimer();
    if (canRunDemoTimer()) runDemoTick();
  }

  function isNativeInteractive(target) {
    return target instanceof Element && Boolean(target.closest('a, button, input, select, textarea, summary, [contenteditable="true"]'));
  }

  function handleDemoKeydown(event) {
    if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || isNativeInteractive(event.target)) return;
    if (event.key === ' ' || event.code === 'Space') {
      event.preventDefault();
      togglePlay();
      return;
    }
    if (!shortcutsEnabled) return;
    if (event.key === '/') {
      event.preventDefault();
      get('tui-filter-input').focus();
      return;
    }
    if (event.key >= '1' && event.key <= '5') {
      switchView(Number(event.key));
      return;
    }
    if (event.key === 't') cycleDemoTheme();
    else if (event.key === 'v') toggleDemoVisualizer();
    else if (event.key === 'l') toggleDemoLyrics();
    else if (event.key === 'R' || (event.shiftKey && event.key === 'r')) triggerDemoRadio();
    else if (event.key === 'n') setCurrentTrackByDirection(1);
    else if (event.key === 'p') setCurrentTrackByDirection(-1);
    else if (event.key === 'Escape') {
      if (showLyrics || showVisualizer) {
        showLyrics = false;
        showVisualizer = false;
        updatePanelViews();
      } else if (activeFilter) {
        clearFilter(event);
      }
    }
  }

  function init() {
    const screen = get('tui-terminal-screen');
    const rows = get('tui-rows-container');
    const shortcutToggle = get('tui-shortcuts-toggle');
    if (!screen || !rows) return;

    screen.addEventListener('keydown', handleDemoKeydown);
    document.querySelectorAll('[role="tablist"]').forEach((list) => {
      list.addEventListener('keydown', (event) => {
        const tabs = Array.from(list.querySelectorAll('[role="tab"]'));
        const index = tabs.indexOf(event.target);
        if (index < 0) return;
        let next;
        if (event.key === 'ArrowRight') next = (index + 1) % tabs.length;
        else if (event.key === 'ArrowLeft') next = (index + tabs.length - 1) % tabs.length;
        else if (event.key === 'Home') next = 0;
        else if (event.key === 'End') next = tabs.length - 1;
        else return;
        event.preventDefault();
        tabs[next].click();
        tabs[next].focus();
      });
    });
    rows.addEventListener('click', (event) => {
      const row = event.target.closest('.tui-track-row');
      if (row) selectAndPlay(row.dataset.trackId);
    });
    shortcutToggle.addEventListener('change', () => {
      shortcutsEnabled = shortcutToggle.checked;
      get('tui-shortcuts-status').innerText = shortcutsEnabled ? 'Shortcuts on' : 'Shortcuts off';
    });
    document.addEventListener('visibilitychange', () => setPageVisibility(!document.hidden));
    if ('IntersectionObserver' in window) {
      const observer = new IntersectionObserver((entries) => {
        if (entries[0]) setDemoVisibility(entries[0].isIntersecting);
      }, { threshold: 0.1 });
      observer.observe(screen);
    }

    updateClock();
    switchView(currentView);
    switchInstallTab(0);
    updatePlayerUI();
    updatePanelViews();
    refreshDemoTimer();
  }

  Object.assign(window, {
    applyDemoTheme,
    clearFilter,
    copyCommand,
    cycleDemoTheme,
    handleFilterInput,
    handleFilterKeydown,
    handleSeekInput,
    handleVolumeChange,
    seekAudio,
    selectAndPlay,
    switchInstallTab,
    switchView,
    toggleDemoLyrics,
    toggleDemoVisualizer,
    togglePlay,
    triggerDemoRadio,
  });

  init();
}());
