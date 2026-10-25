(() => {
  'use strict';
  const root = document.documentElement;
  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  const motionButton = document.querySelector('.motion-button');
  let preference;
  try { preference = localStorage.getItem('tuitify-home-motion'); } catch (_) { preference = null; }
  const enabled = () => preference === 'on' || (preference !== 'off' && !reduced.matches);
  function updateMotion() {
    const active = enabled();
    root.classList.toggle('motion-enabled', active);
    root.classList.toggle('motion-paused', !active || document.hidden);
    root.classList.toggle('motion-override', preference === 'on');
    motionButton.setAttribute('aria-pressed', String(active));
    motionButton.setAttribute('aria-label', active ? 'Pause animations' : 'Enable animations');
    motionButton.firstElementChild.textContent = active ? 'Ⅱ' : '▷';
    motionButton.querySelector('.motion-word').textContent = active ? 'Motion on' : 'Motion off';
    dispatchEvent(new CustomEvent('tuitify:home-motion', { detail: { active } }));
  }
  motionButton.addEventListener('click', () => {
    preference = enabled() ? 'off' : 'on';
    try { localStorage.setItem('tuitify-home-motion', preference); } catch (_) {}
    updateMotion();
  });
  reduced.addEventListener('change', updateMotion);
  document.addEventListener('visibilitychange', updateMotion);
  updateMotion();
  if ('IntersectionObserver' in window) {
    const observer = new IntersectionObserver(entries => entries.forEach(entry => {
      if (entry.isIntersecting) { entry.target.classList.add('in-view'); observer.unobserve(entry.target); }
    }), { threshold: .08, rootMargin: '0px 0px -30px' });
    document.querySelectorAll('[data-reveal]').forEach(element => observer.observe(element));
  }
  const copy = document.getElementById('copy-command');
  copy.addEventListener('click', async () => {
    const status = document.getElementById('copy-status');
    try {
      await navigator.clipboard.writeText('tuitify');
      copy.textContent = 'Copied';
      status.textContent = 'Launch command copied. Open Windows Terminal and paste it.';
    } catch (_) {
      copy.textContent = 'Select command';
      status.textContent = 'Clipboard unavailable. Select “tuitify” above to copy it.';
    }
    setTimeout(() => { copy.textContent = 'Copy'; status.textContent = ''; }, 4000);
  });
})();
