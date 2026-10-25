const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const path = require('node:path');
const { version } = require('../package.json');

const download = `https://github.com/braces157/tutify/releases/download/v${version}/tuitify.exe`;

test('homepage loads local assets without script errors or failed requests', async ({ page }) => {
  const errors = [];
  const remoteAssets = [];
  const failures = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('response', response => { if (response.status() >= 400) failures.push(response.url()); });
  page.on('request', request => {
    if (!request.isNavigationRequest() && new URL(request.url()).origin !== 'http://127.0.0.1:4173') remoteAssets.push(request.url());
  });
  await page.goto('/docs/index.html');
  await page.evaluate(() => document.fonts.ready);
  await page.locator('footer').scrollIntoViewIfNeeded();
  await expect(page.locator('link[href="assets/home.css"]')).toHaveCount(1);
  await expect(page.locator('script[src="assets/home.js"]')).toHaveCount(1);
  for (const image of await page.locator('img').all()) {
    await expect.poll(() => image.evaluate(el => el.complete && el.naturalWidth > 0)).toBe(true);
  }
  expect(errors).toEqual([]);
  expect(remoteAssets).toEqual([]);
  expect(failures).toEqual([]);
});

test('release downloads agree with the manifests and separate source-only features', async ({ page }) => {
  const root = path.resolve(__dirname, '..');
  const lock = JSON.parse(fs.readFileSync(path.join(root, 'package-lock.json'), 'utf8'));
  const cargo = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8');
  expect(cargo.match(/version = "([^"]+)"/)[1]).toBe(version);
  expect(lock.version).toBe(version);
  expect(lock.packages[''].version).toBe(version);
  await page.goto('/docs/index.html');
  const urls = await page.locator('a[href*="/releases/download/"]').evaluateAll(links => links.map(link => link.href));
  expect(urls.length).toBeGreaterThanOrEqual(4);
  for (const url of urls) {
    expect([download, `https://github.com/braces157/tutify/releases/download/v${version}/Tuitify-${version}-windows-x86_64.zip`]).toContain(url);
  }
  await expect(page.locator('.release-boundary')).toContainText('These changes are unreleased.');
  await expect(page.locator('.release-boundary')).toContainText('Monochrome before Volta, Spotify, and YouTube');
  await expect(page.locator('.install-copy')).toContainText('scripts/install.ps1');
  await expect(page.locator('.install-copy')).toContainText('standalone tuitify.exe');
  await expect(page.locator('.install-terminal')).toContainText('After installation on PATH');
});

test('homepage navigation and player link have real destinations', async ({ page }) => {
  await page.goto('/docs/index.html');
  await expect(page.locator('a[href="#"], [onclick*="alert("]')).toHaveCount(0);
  const missing = await page.locator('a[href^="#"]').evaluateAll(links => links.map(a => a.hash).filter(hash => !document.getElementById(hash.slice(1))));
  expect(missing).toEqual([]);
  await expect(page.locator('#interactive-demo')).toBeAttached();
  await page.locator('.demo-link').click();
  await expect(page).toHaveURL(/\/docs\/player.html$/);
  await expect(page.locator('#tui-demo-note')).toContainText('no audio');
  await page.locator('.player-footer a').first().click();
  await expect(page).toHaveURL(/\/docs\/index.html$/);
});

test('native captures disclose their versions and simulated content', async ({ page }) => {
  await page.goto('/docs/index.html');
  await expect(page.locator('.session-footer')).toContainText(`v${version} native demo · fictional tracks`);
  await expect(page.locator('.quiet-visual figcaption')).toContainText('v0.6.0 live capture');
  await expect(page.locator('.faq')).toContainText('Python 3.10+');
  await expect(page.locator('.faq')).toContainText('Discord presence is enabled by default');
});

test('copying the launch command reports success', async ({ page }) => {
  await page.goto('/docs/index.html');
  await page.evaluate(() => Object.defineProperty(navigator, 'clipboard', {
    value: { writeText: async text => { window.copiedCommand = text; } }, configurable: true,
  }));
  await page.locator('#copy-command').click();
  await expect(page.locator('#copy-command')).toHaveText('Copied');
  expect(await page.evaluate(() => window.copiedCommand)).toBe('tuitify');
  await expect(page.locator('#copy-status')).toContainText('Launch command copied');
});

for (const absent of [true, false]) {
  test(`copy fallback explains ${absent ? 'missing clipboard' : 'denied permission'}`, async ({ page }) => {
    await page.goto('/docs/index.html');
    await page.evaluate(missing => Object.defineProperty(navigator, 'clipboard', {
      value: missing ? undefined : { writeText: async () => { throw new Error('denied'); } }, configurable: true,
    }), absent);
    await page.locator('#copy-command').click();
    await expect(page.locator('#copy-command')).toHaveText('Select command');
    await expect(page.locator('#copy-status')).toContainText('Clipboard unavailable');
    await expect(page.locator('.terminal-command code')).toHaveText('tuitify');
  });
}

test('homepage motion follows reduced motion and persists explicit choices', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await page.goto('/docs/index.html');
  const toggle = page.locator('.motion-button');
  await expect(toggle).toHaveAccessibleName('Motion');
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await page.reload();
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await toggle.click();
  await expect(page.locator('html')).toHaveClass(/motion-paused/);
  await page.reload();
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await expect(toggle).toHaveAccessibleName('Motion');
});

test('record animation pauses on request and while offscreen', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await page.goto('/docs/index.html');
  const record = page.locator('.record-grooves');
  const pause = page.locator('.record-motion');
  await expect(pause).toBeVisible();
  await pause.click();
  await expect(pause).toHaveAttribute('aria-pressed', 'true');
  await expect(pause).toHaveAccessibleName('Pause record animation');
  await expect(record).toHaveCSS('animation-play-state', 'paused');
  await pause.click();
  await expect(record).toHaveCSS('animation-play-state', 'running');
  await expect(pause).toHaveAccessibleName('Pause record animation');
  await page.locator('footer').scrollIntoViewIfNeeded();
  await expect(record).toHaveCSS('animation-play-state', 'paused');
});

for (const width of [320, 390, 768, 1024, 1440, 1920]) {
  test(`homepage sections fit at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto('/docs/index.html');
    await page.evaluate(() => document.fonts.ready);
    for (const selector of ['.hero', '#player', '#features', '#install', '.release-boundary', '.footer']) {
      await page.locator(selector).scrollIntoViewIfNeeded();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), selector).toBe(true);
    }
    for (const control of await page.locator('.site-header a, .site-header button, .hero-actions a').all()) {
      if (!await control.isVisible()) continue;
      const bounds = await control.boundingBox();
      expect(bounds.x).toBeGreaterThanOrEqual(0);
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(width + 1);
    }
  });
}

test('homepage content, install guidance and downloads work without JavaScript', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto('http://127.0.0.1:4173/docs/index.html');
  await expect(page.locator('h1')).toBeVisible();
  await expect(page.locator('.primary-button').first()).toHaveAttribute('href', download);
  await expect(page.locator('.install-copy')).toBeVisible();
  await expect(page.locator('.release-boundary')).toBeVisible();
  await page.locator('.faq summary').first().click();
  await expect(page.locator('.faq details').first()).toHaveAttribute('open', '');
  await context.close();
});


test('reveal content stays readable without IntersectionObserver', async ({ page }) => {
  await page.addInitScript(() => { delete window.IntersectionObserver; });
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await page.goto('/docs/index.html');
  for (const panel of await page.locator('[data-reveal]').all()) {
    await expect(panel).toHaveCSS('opacity', '1');
  }
  const motion = page.locator('.motion-button');
  await motion.click();
  await expect(page.locator('.record-motion')).toBeDisabled();
  await expect(page.locator('.record-motion')).toHaveAccessibleDescription('Record animation off; enable motion in the header');
});
