(function () {
  'use strict';
  const root = document.documentElement;
  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
  const toggle = document.querySelector('.motion-toggle');
  const record = document.querySelector('.record-scene');
  let modeAnimations = [];
  let motionPreference = null;
  try { motionPreference = localStorage.getItem('tuitify-motion'); } catch (_) { /* Storage may be unavailable. */ }
  // Follow the device by default; an explicit choice belongs to the visitor.
  const motionEnabled = () => motionPreference === 'on' || (motionPreference !== 'off' && !reducedMotion.matches);

  // Split headings into words before reveal classes are applied, so words can rise in sequence.
  document.querySelectorAll('[data-split]').forEach((heading) => {
    let index = 0;
    const walker = document.createTreeWalker(heading, NodeFilter.SHOW_TEXT);
    const nodes = [];
    while (walker.nextNode()) nodes.push(walker.currentNode);
    nodes.forEach((node) => {
      const parts = node.textContent.split(/(\s+)/);
      if (!parts.some(part => part.trim())) return;
      const fragment = document.createDocumentFragment();
      parts.forEach((part) => {
        if (!part) return;
        if (!part.trim()) { fragment.appendChild(document.createTextNode(part)); return; }
        const outer = document.createElement('span');
        outer.className = 'split-word';
        const inner = document.createElement('span');
        inner.className = 'split-inner';
        inner.style.setProperty('--w', String(index++));
        inner.textContent = part;
        outer.appendChild(inner);
        fragment.appendChild(outer);
      });
      node.replaceWith(fragment);
    });
  });

  function updateMotion() {
    const enabled = motionEnabled();
    root.classList.toggle('motion-enabled', enabled);
    root.classList.toggle('motion-override', motionPreference === 'on');
    root.classList.toggle('motion-paused', !enabled || document.hidden);
    toggle.setAttribute('aria-pressed', String(enabled));
    toggle.disabled = false;
    toggle.setAttribute('aria-label', enabled ? 'Pause website animations' : 'Enable website animations');
    toggle.title = !enabled && reducedMotion.matches && motionPreference !== 'off'
      ? 'Your device prefers reduced motion. Click to enable animations for this website.'
      : enabled ? 'Pause website animations' : 'Enable website animations';
    toggle.querySelector('.motion-label').textContent = enabled ? 'Motion on' : 'Motion off';
    toggle.querySelector('.motion-icon').textContent = enabled ? 'Ⅱ' : '▷';
    if (!enabled) modeAnimations.forEach(animation => animation.cancel());
    document.dispatchEvent(new CustomEvent('tuitify:motion-change', { detail: { enabled } }));
  }
  toggle.addEventListener('click', () => {
    motionPreference = motionEnabled() ? 'off' : 'on';
    try { localStorage.setItem('tuitify-motion', motionPreference); } catch (_) { /* Keep the setting for this visit. */ }
    updateMotion();
  });
  reducedMotion.addEventListener('change', updateMotion);
  document.addEventListener('visibilitychange', updateMotion);

  // Count-up numbers in the hero stats.
  function runCounters(scope) {
    scope.querySelectorAll('[data-count]').forEach((element) => {
      const target = Number(element.dataset.count);
      const from = Number(element.dataset.countFrom || 0);
      if (!motionEnabled() || target === from) { element.textContent = String(target); return; }
      const start = performance.now();
      const duration = 1400;
      const step = (now) => {
        const progress = Math.min(1, (now - start) / duration);
        const eased = 1 - Math.pow(1 - progress, 4);
        element.textContent = String(Math.round(from + (target - from) * eased));
        if (progress < 1) window.requestAnimationFrame(step);
      };
      window.requestAnimationFrame(step);
    });
  }

  const revealTargets = document.querySelectorAll('.reveal, [data-split], .footer-top');
  if ('IntersectionObserver' in window) {
    const reveals = new IntersectionObserver((entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          entry.target.classList.add('is-visible');
          if (entry.target.querySelector('[data-count]')) runCounters(entry.target);
          reveals.unobserve(entry.target);
        }
      });
    }, { threshold: .08, rootMargin: '0px 0px -6% 0px' });
    revealTargets.forEach((element) => reveals.observe(element));
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
  // Keep the content visible if this browser cannot observe reveal targets.
  if (!('IntersectionObserver' in window)) revealTargets.forEach(element => element.classList.add('is-visible'));
  updateMotion();

  // Header becomes denser once the page scrolls.
  const header = document.querySelector('.site-header');
  let headerTicking = false;
  const updateHeader = () => { headerTicking = false; header.classList.toggle('is-scrolled', window.scrollY > 24); };
  window.addEventListener('scroll', () => { if (!headerTicking) { headerTicking = true; window.requestAnimationFrame(updateHeader); } }, { passive: true });
  updateHeader();

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
    if (open && motionEnabled()) menu.animate([{ opacity: 0, transform: 'translateY(-8px) scale(.98)' }, { opacity: 1, transform: 'none' }], { duration: 320, easing: 'cubic-bezier(.22,1,.36,1)' });
  });
  menu.addEventListener('click', (event) => { if (event.target.closest('a')) closeMenu(); });
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && !menu.hidden) closeMenu(true);
  });
  const mobile = window.matchMedia('(max-width: 860px)');
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
    const description = document.getElementById('signal-description');
    description.textContent = content.description;
    stage.querySelector('.signal-indicator').textContent = content.caption;
    stage.querySelectorAll('.signal-title > span').forEach((line, index) => {
      line.textContent = content.lines[index];
      if (changed && motionEnabled()) {
        modeAnimations.push(line.animate([{ opacity: 0, transform: 'translateY(30px)', filter: 'blur(8px)' }, { opacity: 1, transform: 'translateY(0)', filter: 'blur(0)' }], { duration: 700, delay: index * 80, easing: 'cubic-bezier(.22,1,.36,1)', fill: 'backwards' }));
      }
    });
    if (changed && motionEnabled()) {
      modeAnimations.push(description.animate([{ opacity: 0, transform: 'translateY(12px)' }, { opacity: 1, transform: 'none' }], { duration: 600, delay: 180, easing: 'cubic-bezier(.22,1,.36,1)', fill: 'backwards' }));
    }
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
    screen.scrollIntoView({ behavior: motionEnabled() ? 'smooth' : 'instant', block: 'center' });
    screen.focus({ preventScroll: true });
    actions[key.dataset.demoAction]();
  }));

  // Light up the matching keycap when a visitor presses the key anywhere on the page (visual only).
  const keycaps = new Map(Array.from(document.querySelectorAll('.key-demo[data-key]')).map(key => [key.dataset.key, key]));
  document.addEventListener('keydown', (event) => {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    const keycap = keycaps.get(event.key);
    if (!keycap) return;
    keycap.classList.add('is-pressed');
    window.setTimeout(() => keycap.classList.remove('is-pressed'), 180);
  });
})();
