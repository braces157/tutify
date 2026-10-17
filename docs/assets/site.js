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

  // Choreograph each section in reading order instead of fading one large container.
  document.querySelectorAll('.story-copy, .keyboard-copy, .install-copy, .questions-heading, .hero-stats, .setup-steps').forEach(group => {
    group.classList.remove('reveal');
    Array.from(group.children).forEach((child, index) => {
      if (child.matches('[data-split]')) return;
      child.classList.add('reveal');
      child.style.setProperty('--i', String(index));
    });
  });
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
    }, { threshold: .02, rootMargin: '0px 0px 64px 0px' });
    revealTargets.forEach((element) => reveals.observe(element));
    const records = new IntersectionObserver((entries) => {
      record.classList.toggle('motion-offscreen', !entries[0].isIntersecting);
    });
    records.observe(record);
    const links = Array.from(document.querySelectorAll('.desktop-nav a[href^="#"]'));
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

  // Native captures are navigable, and always link to their full image.
  const previewTabs = Array.from(document.querySelectorAll('[data-preview-mode]'));
  const previewPanel = document.getElementById('preview-panel');
  const previewImage = previewPanel.querySelector('img');
  const previewLink = previewPanel.querySelector('a');
  const previewError = previewPanel.querySelector('.preview-error');
  const previews = {
    queue: { src: 'assets/screenshots/v0.3.0/glass-queue-smart-shuffle.png', label: 'queue', alt: 'Tuitify queue with Smart Shuffle, artist names, track durations and playback controls in the Glass theme', caption: 'Queue and Smart Shuffle. Native player, Glass theme, v0.3.0.' },
    lyrics: { src: 'assets/screenshots/v0.6.0/glass-lyrics-readings-volta.png', label: 'lyrics and readings', alt: 'Tuitify v0.6.0 Glass theme with Japanese lyrics, romaji readings, and Volta FLAC audio at 24-bit / 96 kHz', caption: 'Lyric readings and Volta FLAC, 24-bit / 96 kHz. Native player, Glass theme, v0.6.0.' },
    spectrum: { src: 'assets/screenshots/v0.3.0/glass-visualizer.jpg', label: 'spectrum', alt: 'Tuitify’s spectrum visualizer beside the native music player controls', caption: 'Audio spectrum. Native player, Glass theme, v0.3.0.' },
  };
  let previewAnimation;
  function selectPreview(tab) {
    const content = previews[tab.dataset.previewMode];
    if (previewAnimation) previewAnimation.cancel();
    previewTabs.forEach(other => {
      const selected = other === tab;
      other.setAttribute('aria-selected', String(selected));
      other.tabIndex = selected ? 0 : -1;
    });
    previewPanel.setAttribute('aria-labelledby', tab.id);
    previewError.hidden = true;
    previewLink.href = content.src;
    previewLink.setAttribute('aria-label', `View the full ${content.label} screenshot`);
    previewImage.alt = content.alt;
    previewImage.src = content.src;
    previewPanel.querySelector('figcaption').textContent = content.caption;
    if (motionEnabled()) previewAnimation = previewLink.animate([
      { opacity: .45, transform: 'translateY(16px)' },
      { opacity: 1, transform: 'translateY(0)' },
    ], { duration: 420, easing: 'cubic-bezier(.22,1,.36,1)' });
  }
  previewTabs.forEach((tab, index) => {
    tab.addEventListener('click', () => selectPreview(tab));
    tab.addEventListener('keydown', event => {
      let next;
      if (event.key === 'ArrowRight') next = (index + 1) % previewTabs.length;
      else if (event.key === 'ArrowLeft') next = (index + previewTabs.length - 1) % previewTabs.length;
      else if (event.key === 'Home') next = 0;
      else if (event.key === 'End') next = previewTabs.length - 1;
      else return;
      event.preventDefault();
      previewTabs[next].focus();
      selectPreview(previewTabs[next]);
    });
  });
  previewImage.addEventListener('error', () => { previewError.hidden = false; });
  previewImage.addEventListener('load', () => { previewError.hidden = true; });
  document.addEventListener('tuitify:motion-change', event => {
    if (!event.detail.enabled && previewAnimation) previewAnimation.cancel();
  });
  document.querySelector('.preview-tabs').hidden = false;

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

  document.querySelectorAll('[data-copy-command]').forEach(button => button.addEventListener('click', async () => {
    const originalContent = button.innerHTML;
    const originalLabel = button.getAttribute('aria-label');
    try {
      await navigator.clipboard.writeText(button.dataset.copyCommand);
      button.textContent = 'Copied';
      button.setAttribute('aria-label', 'Command copied');
    } catch (_) {
      window.switchInstallTab(1);
      document.getElementById('install').scrollIntoView({ behavior: 'instant' });
      button.textContent = 'Select command in the install panel';
      button.setAttribute('aria-label', 'Copy unavailable; select the command in the install panel');
    }
    window.setTimeout(() => {
      button.innerHTML = originalContent;
      button.setAttribute('aria-label', originalLabel);
    }, 3500);
  }));
  const actions = {
    play: () => window.togglePlay(), theme: () => window.cycleDemoTheme(),
    lyrics: () => window.toggleDemoLyrics(), spectrum: () => window.toggleDemoVisualizer(),
    radio: () => window.triggerDemoRadio(), search: () => { window.switchView(1); document.getElementById('tui-filter-input').focus({ preventScroll: true }); },
    next: () => window.setCurrentTrackByDirection(1), queue: () => window.switchView(4),
  };
  document.querySelectorAll('[data-demo-action]').forEach(button => button.addEventListener('click', () => {
    const screen = document.getElementById('tui-terminal-screen');
    disclosure.open = true;
    screen.scrollIntoView({ behavior: 'instant', block: 'center' });
    screen.focus({ preventScroll: true });
    actions[button.dataset.demoAction]();
    document.getElementById('key-action-status').textContent = 'Applied to the silent browser demo.';
  }));
  document.querySelectorAll('.terminal-showcase div').forEach((bar, index) => {
    if (/h-\[\d+%\]/.test(bar.className) && bar.classList.contains('bg-electric-neon')) {
      bar.classList.add('preview-bar');
      bar.style.setProperty('--bar-delay', (index % 9) * -.13 + 's');
    }
  });
  if ('IntersectionObserver' in window) {
    const observer = new IntersectionObserver(entries => entries.forEach(entry => entry.target.classList.toggle('preview-offscreen', !entry.isIntersecting)));
    observer.observe(document.querySelector('.terminal-showcase'));
  }
})();
