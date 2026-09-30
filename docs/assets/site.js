(function () {
  'use strict';
  const root = document.documentElement;
  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
  const toggle = document.querySelector('.motion-toggle');
  const record = document.querySelector('.record-scene');
  let modeAnimations = [];
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
    if (!enabled) modeAnimations.forEach(animation => animation.cancel());
    document.dispatchEvent(new CustomEvent('tuitify:motion-change', { detail: { enabled } }));
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

  const disclosure = document.getElementById('demo-disclosure');
  document.querySelectorAll('a[href="#interactive-demo"]').forEach(link => link.addEventListener('click', () => { disclosure.open = true; }));
  const stage = document.getElementById('signal-stage');
  const signalPanel = document.getElementById('signal-panel');
  const modes = {
    queue: { lines: ['KEEP IT', 'FLOWING.'], description: 'A queue that follows your lead. Add, reorder, undo. Leave room for the next good song.', caption: 'Your queue. Your call.' },
    lyrics: { lines: ['KNOW EVERY', 'WORD.'], description: 'Lyrics that move with the music. Follow timed lines from LRCLIB, or scroll through plain lyrics when available.', caption: 'Stay with the song.' },
    spectrum: { lines: ['FEEL THE', 'FREQUENCY.'], description: 'A live spectrum, drawn from the audio playing in Tuitify. See the rhythm, from the low end to the high notes.', caption: 'A little atmosphere.' },
  };
  const signalTabs = Array.from(document.querySelectorAll('[data-signal-mode]'));
  signalTabs.forEach(tab => tab.addEventListener('click', () => {
    const selectedMode = tab.dataset.signalMode;
    const changed = stage.dataset.mode !== selectedMode;
    stage.dataset.mode = selectedMode;
    signalTabs.forEach(other => {
      const active = other === tab;
      other.setAttribute('aria-selected', String(active));
      other.tabIndex = active ? 0 : -1;
    });
    signalPanel.setAttribute('aria-labelledby', tab.id);
    const content = modes[selectedMode];
    modeAnimations.forEach(animation => animation.cancel());
    modeAnimations = [];
    document.getElementById('signal-description').textContent = content.description;
    stage.querySelector('.signal-indicator').textContent = content.caption;
    stage.querySelectorAll('.signal-title > span').forEach((line, index) => {
      line.textContent = content.lines[index];
      if (changed && motionOn && !reducedMotion.matches) {
        modeAnimations.push(line.animate([{ opacity: 0, transform: 'translateY(25px)' }, { opacity: 1, transform: 'translateY(0)' }], { duration: 650, delay: index * 65, easing: 'cubic-bezier(.22,1,.36,1)' }));
      }
    });
    modeAnimations = modeAnimations.filter(animation => animation.playState !== 'finished');
    document.dispatchEvent(new CustomEvent('tuitify:signal-change'));
  }));

  const actions = {
    play: () => window.togglePlay(), theme: () => window.cycleDemoTheme(),
    lyrics: () => window.toggleDemoLyrics(), spectrum: () => window.toggleDemoVisualizer(),
    radio: () => window.triggerDemoRadio(), search: () => { window.switchView(1); document.getElementById('tui-filter-input').focus({ preventScroll: true }); },
    next: () => window.setCurrentTrackByDirection(1), previous: () => window.setCurrentTrackByDirection(-1),
  };
  document.querySelectorAll('[data-demo-action]').forEach((key) => key.addEventListener('click', () => {
    const screen = document.getElementById('tui-terminal-screen');
    disclosure.open = true;
    screen.scrollIntoView({ behavior: motionOn && !reducedMotion.matches ? 'smooth' : 'instant', block: 'center' });
    screen.focus({ preventScroll: true });
    actions[key.dataset.demoAction]();
  }));
})();
