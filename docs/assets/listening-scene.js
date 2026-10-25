(() => {
  'use strict';
  const scene = document.querySelector('.listening-scene');
  const control = scene?.querySelector('.record-motion');
  if (!scene || !control) return;

  const root = document.documentElement;
  let paused = false;

  function update() {
    const motionEnabled = root.classList.contains('motion-enabled');
    const stopped = paused || !motionEnabled;
    scene.classList.toggle('record-paused', paused);
    control.disabled = !motionEnabled;
    control.setAttribute('aria-pressed', String(stopped));
    control.setAttribute('aria-description', motionEnabled
      ? 'Pause or resume the spinning record.'
      : 'Record animation off; enable motion in the header');
    control.querySelector('span').textContent = motionEnabled
      ? (paused ? 'Resume motion' : 'Pause motion')
      : 'Motion off';
  }

  control.hidden = false;
  control.addEventListener('click', () => {
    paused = !paused;
    update();
  });
  addEventListener('tuitify:home-motion', update);

  if ('IntersectionObserver' in window) {
    const observer = new IntersectionObserver(entries => {
      scene.classList.toggle('is-visible', entries[0].isIntersecting);
    }, { threshold: .05 });
    observer.observe(scene);
  } else {
    scene.classList.add('is-visible');
  }
  update();
})();
