(function () {
  'use strict';
  const root = document.documentElement;
  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
  const toggle = document.querySelector('.motion-toggle');
  const record = document.querySelector('.record-scene');
  let motionPreference = null;
  try { motionPreference = localStorage.getItem('tuitify-motion'); } catch (_) { /* Storage may be unavailable. */ }
  let motionOn = motionPreference === null ? !reducedMotion.matches : motionPreference === 'on';

  function updateMotion() {
    const enabled = motionOn && !reducedMotion.matches;
    root.classList.toggle('motion-enabled', enabled);
    root.classList.toggle('motion-paused', !enabled || document.hidden);
    toggle.setAttribute('aria-pressed', String(enabled));
    toggle.disabled = reducedMotion.matches;
    toggle.setAttribute('aria-label', reducedMotion.matches ? 'Animations off: reduced motion preference' : enabled ? 'Pause website animations' : 'Enable website animations');
    toggle.querySelector('.motion-label').textContent = reducedMotion.matches ? 'Reduced motion' : enabled ? 'Motion on' : 'Motion off';
    toggle.querySelector('.motion-icon').textContent = enabled ? 'Ⅱ' : '▷';
  }
  toggle.addEventListener('click', () => {
    motionOn = !(motionOn && !reducedMotion.matches);
    try { localStorage.setItem('tuitify-motion', motionOn ? 'on' : 'off'); } catch (_) { /* Keep the setting for this visit. */ }
    updateMotion();
  });
  reducedMotion.addEventListener('change', updateMotion);
  document.addEventListener('visibilitychange', updateMotion);

  if ('IntersectionObserver' in window) {
    const reveals = new IntersectionObserver((entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          entry.target.classList.add('is-visible');
          reveals.unobserve(entry.target);
        }
      });
    }, { threshold: .08 });
    document.querySelectorAll('.reveal').forEach((element) => reveals.observe(element));
    const records = new IntersectionObserver((entries) => {
      record.classList.toggle('motion-offscreen', !entries[0].isIntersecting);
    });
    records.observe(record);
    const links = Array.from(document.querySelectorAll('.desktop-nav a'));
    const sections = links.map((link) => document.querySelector(link.getAttribute('href'))).filter(Boolean);
    const activeSections = new Map();
    const navigation = new IntersectionObserver((entries) => {
      entries.forEach((entry) => activeSections.set(entry.target.id, entry.isIntersecting));
      const active = sections.find((section) => activeSections.get(section.id));
      links.forEach((link) => {
        if (active && link.hash === `#${active.id}`) link.setAttribute('aria-current', 'location');
        else link.removeAttribute('aria-current');
      });
    }, { rootMargin: '-15% 0px -55% 0px' });
    sections.forEach((section) => navigation.observe(section));
  }
  // A no-JavaScript page is visible by default; only enable reveals after observing.
  if ('IntersectionObserver' in window) updateMotion();

  const menuButton = document.querySelector('.menu-toggle');
  const menu = document.getElementById('mobile-nav');
  function closeMenu(returnFocus = false) {
    menu.hidden = true;
    menuButton.setAttribute('aria-expanded', 'false');
    menuButton.setAttribute('aria-label', 'Open navigation');
    if (returnFocus) menuButton.focus();
  }
  menuButton.addEventListener('click', () => {
    const open = menuButton.getAttribute('aria-expanded') !== 'true';
    menu.hidden = !open;
    menuButton.setAttribute('aria-expanded', String(open));
    menuButton.setAttribute('aria-label', open ? 'Close navigation' : 'Open navigation');
  });
  menu.addEventListener('click', (event) => { if (event.target.closest('a')) closeMenu(); });
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && !menu.hidden) closeMenu(true);
  });
  const mobile = window.matchMedia('(max-width: 700px)');
  mobile.addEventListener('change', () => { if (!mobile.matches) closeMenu(); });

  const shots = {
    queue: { src: 'assets/screenshots/v0.3.0/glass-queue-smart-shuffle.png', alt: "Tuitify's Glass theme showing a queue with Smart Shuffle suggestions over a wallpaper", caption: 'Your queue, with room for a few good discoveries.' },
    lyrics: { src: 'assets/screenshots/v0.3.0/glass-lyrics.jpg', alt: "Synchronized lyrics in Tuitify's Glass theme with the current line highlighted", caption: 'Every word, right where the music is.' },
    spectrum: { src: 'assets/screenshots/v0.3.0/glass-visualizer.jpg', alt: "Tuitify's real-time audio spectrum displayed in the Glass terminal theme", caption: 'A little atmosphere. Driven by the actual audio.' },
  };
  const shotTabs = Array.from(document.querySelectorAll('[data-shot]'));
  const panel = document.getElementById('screenshot-panel');
  const preview = document.getElementById('product-screenshot');
  const previewError = panel.querySelector('.screenshot-error');
  preview.addEventListener('load', () => { panel.classList.remove('is-loading'); previewError.hidden = true; });
  preview.addEventListener('error', () => { panel.classList.remove('is-loading'); previewError.hidden = false; });
  shotTabs.forEach((tab) => tab.addEventListener('click', () => {
    const shot = shots[tab.dataset.shot];
    const previous = panel.getAttribute('aria-labelledby');
    shotTabs.forEach((other) => {
      const selected = other === tab;
      other.setAttribute('aria-selected', String(selected));
      other.tabIndex = selected ? 0 : -1;
    });
    panel.setAttribute('aria-labelledby', tab.id);
    document.getElementById('screenshot-caption').textContent = shot.caption;
    if (previous === tab.id) return;
    previewError.hidden = true;
    panel.classList.add('is-loading');
    preview.alt = shot.alt;
    preview.src = shot.src;
    if (preview.complete) {
      panel.classList.remove('is-loading');
      previewError.hidden = preview.naturalWidth > 0;
    }
  }));
  // Lazy images report no dimensions until requested; an error event handles failures.

  const actions = {
    play: () => window.togglePlay(), theme: () => window.cycleDemoTheme(),
    lyrics: () => window.toggleDemoLyrics(), spectrum: () => window.toggleDemoVisualizer(),
    radio: () => window.triggerDemoRadio(), search: () => { window.switchView(1); document.getElementById('tui-filter-input').focus({ preventScroll: true }); },
    next: () => window.setCurrentTrackByDirection(1), previous: () => window.setCurrentTrackByDirection(-1),
  };
  document.querySelectorAll('[data-demo-action]').forEach((key) => key.addEventListener('click', () => {
    const screen = document.getElementById('tui-terminal-screen');
    screen.scrollIntoView({ behavior: motionOn && !reducedMotion.matches ? 'smooth' : 'instant', block: 'center' });
    screen.focus({ preventScroll: true });
    actions[key.dataset.demoAction]();
  }));
})();
