(function () {
  'use strict';
  const root = document.documentElement;
  const finePointer = window.matchMedia('(hover: hover) and (pointer: fine)');
  const reduce = window.matchMedia('(prefers-reduced-motion: reduce)');
  const allowed = () => !reduce.matches && !document.hidden && !root.classList.contains('motion-paused');
  const record = document.querySelector('.record-scene');
  const magnets = Array.from(document.querySelectorAll('.button-primary'));
  let pointerFrame = 0;
  let pointerTarget = null;
  let pointerX = 0;
  let pointerY = 0;

  function resetPointerEffects() {
    window.cancelAnimationFrame(pointerFrame);
    pointerFrame = 0;
    pointerTarget = null;
    record.classList.remove('is-hovered');
    ['--tilt-x', '--tilt-y', '--float-x', '--float-y', '--shine-x'].forEach(name => record.style.removeProperty(name));
    magnets.forEach(button => { button.style.removeProperty('--magnet-x'); button.style.removeProperty('--magnet-y'); });
  }
  function movePointer() {
    pointerFrame = 0;
    if (!pointerTarget || !allowed() || !finePointer.matches) return;
    if (pointerTarget === record) {
      record.style.setProperty('--tilt-x', `${(-pointerY * 9).toFixed(2)}deg`);
      record.style.setProperty('--tilt-y', `${(pointerX * 12).toFixed(2)}deg`);
      record.style.setProperty('--float-x', `${(pointerX * 8 - 7).toFixed(2)}px`);
      record.style.setProperty('--float-y', `${(pointerY * 7).toFixed(2)}px`);
      record.style.setProperty('--shine-x', `${(pointerX * 28).toFixed(2)}%`);
    } else {
      pointerTarget.style.setProperty('--magnet-x', `${(pointerX * 5).toFixed(2)}px`);
      pointerTarget.style.setProperty('--magnet-y', `${(pointerY * 4).toFixed(2)}px`);
    }
  }
  [record, ...magnets].forEach(element => {
    let bounds;
    element.addEventListener('pointerenter', () => {
      bounds = element.getBoundingClientRect();
      if (element === record && allowed() && finePointer.matches) record.classList.add('is-hovered');
    });
    element.addEventListener('pointermove', event => {
      if (!allowed() || !finePointer.matches || !bounds) return;
      pointerTarget = element;
      pointerX = Math.max(-1, Math.min(1, (event.clientX - bounds.left) / bounds.width * 2 - 1));
      pointerY = Math.max(-1, Math.min(1, (event.clientY - bounds.top) / bounds.height * 2 - 1));
      if (!pointerFrame) pointerFrame = window.requestAnimationFrame(movePointer);
    });
    element.addEventListener('pointerleave', resetPointerEffects);
  });

  const stage = document.getElementById('signal-stage');
  const canvas = document.getElementById('signal-canvas');
  const ctx = canvas.getContext('2d', { alpha: true });
  let visible = false;
  let frame = 0;
  let lastPaint = 0;
  let lastTime = 0;
  let phase = 0;
  let width = 0;
  let height = 0;
  let pointerBounds;
  let targetX = 0;
  let targetY = 0;
  let smoothX = 0;
  let smoothY = 0;
  let pulse = 0;
  let targetMode = 0;
  let mode = 0;

  function draw() {
    if (!ctx || !width || !height) return;
    const mobile = width < 600;
    const radius = Math.min(width * (mobile ? .28 : .245), height * (mobile ? .225 : .41));
    const centerX = width * (mobile ? .52 : .765);
    const centerY = height * (mobile ? .72 : .52);
    const rings = mobile ? 36 : 56;
    const segments = mobile ? 52 : 72;
    const angleX = .95 + Math.sin(phase * .22) * .16 + smoothY * .3;
    const angleY = -.4 + phase * .13 + smoothX * .45;
    const cx = Math.cos(angleX), sx = Math.sin(angleX);
    const cy = Math.cos(angleY), sy = Math.sin(angleY);
    ctx.clearRect(0, 0, width, height);
    const paths = [];
    for (let ring = 0; ring < rings; ring += 1) {
      const theta = ring / rings * Math.PI * 2;
      const wobble = Math.sin(theta * (3 + mode) + phase * .6) * (.055 + mode * .022 + pulse * .05);
      const major = radius * (.77 + wobble);
      const minor = radius * (.27 + Math.sin(theta * 4 - phase * .35) * .06);
      const points = [];
      let depth = 0;
      for (let segment = 0; segment <= segments; segment += 1) {
        const phi = segment / segments * Math.PI * 2;
        const r = major + minor * Math.cos(phi);
        const x = r * Math.cos(theta);
        const y = r * Math.sin(theta);
        const z = minor * Math.sin(phi) + Math.sin(theta * 2 + phase * .4) * radius * .12;
        const yy = y * cx - z * sx;
        const zz = y * sx + z * cx;
        const xx = x * cy + zz * sy;
        const zzz = -x * sy + zz * cy;
        const perspective = 1 / (1 + zzz / (radius * 4));
        points.push([centerX + xx * perspective, centerY + yy * perspective]);
        depth += zzz;
      }
      paths.push({ points, depth: depth / segments });
    }
    paths.sort((a, b) => b.depth - a.depth);
    paths.forEach(({ points, depth }) => {
      const light = Math.max(.14, Math.min(.84, .47 - depth / radius * .42));
      ctx.beginPath();
      points.forEach(([x, y], index) => { if (index === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y); });
      ctx.strokeStyle = `rgba(184,232,135,${light.toFixed(3)})`;
      ctx.lineWidth = light > .65 ? 1.2 : .65;
      ctx.stroke();
    });
  }
  function tick(time) {
    frame = 0;
    if (!allowed() || !visible || !ctx) return;
    if (time - lastPaint >= 1000 / 30) {
      const dt = lastTime ? Math.min((time - lastTime) / 1000, .08) : 0;
      lastTime = time;
      lastPaint = time;
      phase += dt;
      smoothX += (targetX - smoothX) * .085;
      smoothY += (targetY - smoothY) * .085;
      mode += (targetMode - mode) * .06;
      pulse *= .94;
      draw();
    }
    frame = window.requestAnimationFrame(tick);
  }
  function refresh() {
    window.cancelAnimationFrame(frame);
    frame = 0;
    lastTime = 0;
    if (allowed() && visible && ctx) frame = window.requestAnimationFrame(tick);
    else {
      smoothX = 0;
      smoothY = 0;
      mode = targetMode;
      draw();
    }
    if (!allowed()) resetPointerEffects();
  }
  function resize() {
    width = stage.clientWidth;
    height = stage.clientHeight;
    const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    if (ctx) ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    draw();
    refresh();
  }
  if (ctx) {
    stage.classList.add('has-canvas');
    if ('ResizeObserver' in window) new ResizeObserver(resize).observe(stage);
    else window.addEventListener('resize', resize, { passive: true });
    resize();
  }
  stage.addEventListener('pointerenter', () => { pointerBounds = stage.getBoundingClientRect(); });
  stage.addEventListener('pointermove', event => {
    if (!allowed() || !finePointer.matches || !pointerBounds) return;
    targetX = (event.clientX - pointerBounds.left) / pointerBounds.width * 2 - 1;
    targetY = (event.clientY - pointerBounds.top) / pointerBounds.height * 2 - 1;
  });
  stage.addEventListener('pointerleave', () => { targetX = 0; targetY = 0; });
  stage.addEventListener('pointerdown', event => {
    if (allowed() && !event.target.closest('button')) pulse = 1;
  });
  document.addEventListener('tuitify:signal-change', () => {
    targetMode = { queue: 0, lyrics: 1, spectrum: 2 }[stage.dataset.mode] || 0;
    pulse = allowed() ? 1 : 0;
    if (!allowed()) refresh();
  });
  document.addEventListener('tuitify:motion-change', refresh);
  document.addEventListener('visibilitychange', refresh);
  finePointer.addEventListener('change', resetPointerEffects);
  if ('IntersectionObserver' in window) {
    const observer = new IntersectionObserver(entries => {
      entries.forEach(entry => {
        if (entry.target === stage) { visible = entry.isIntersecting; refresh(); }
        else entry.target.classList.toggle('motion-offscreen', !entry.isIntersecting);
      });
    }, { threshold: .04 });
    observer.observe(stage);
    observer.observe(document.querySelector('.kinetic-band'));
  } else { visible = true; refresh(); }
  window.addEventListener('pagehide', () => { window.cancelAnimationFrame(frame); resetPointerEffects(); });
  window.addEventListener('pageshow', refresh);
})();
