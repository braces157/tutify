const { test, expect } = require('@playwright/test');
const { version: releaseVersion } = require('../package.json');

async function openDemo(page) {
  await page.goto('/docs/index.html');
  await page.locator('#interactive-demo').scrollIntoViewIfNeeded();
  await page.locator('#demo-disclosure summary').click();
  await expect(page.locator('#tui-terminal-screen')).toBeVisible();
}

test('uses local compiled assets without parser-blocking CSS runtimes', async ({ page }) => {
  const remoteRequests = [];
  page.on('request', (request) => {
    if (request.isNavigationRequest()) return;
    const url = request.url();
    if (new URL(url).origin !== 'http://127.0.0.1:4173') remoteRequests.push(url);
  });
  await page.goto('/docs/index.html');
  await expect(page.locator('link[href^="assets/tailwind.compiled.css"]')).toHaveCount(1);
  await expect(page.locator('script[src="assets/demo.js"]')).toHaveCount(1);
  expect(remoteRequests).toEqual([]);
});

test('all demo tabs expose different supported content', async ({ page }) => {
  await openDemo(page);
  await expect(page.locator('#tui-table-title')).toContainText('LIKED SONGS');
  await page.locator('#tui-tab-1').click();
  await expect(page.locator('#tui-table-title')).toContainText('SEARCH RESULTS');
  await page.locator('#tui-tab-2').click();
  await expect(page.locator('#tui-table-title')).toContainText('PLAYLISTS');
  await page.locator('#tui-tab-4').click();
  await expect(page.locator('#tui-table-title')).toContainText('QUEUE');
  await page.locator('#tui-tab-5').click();
  await expect(page.locator('#tui-help-view')).toContainText('Focus the terminal panel');
  await expect(page.locator('#tui-catalog-view')).toBeHidden();
});

test('radio reports the actual number of additions and remains idempotent', async ({ page }) => {
  await openDemo(page);
  await page.locator('#tui-radio-btn').click();
  await expect(page.locator('#tui-toast')).toContainText('queued 6 related tracks');
  await expect(page.locator('#tui-queue-count')).toHaveText('14');
  await page.locator('#tui-radio-btn').click();
  await expect(page.locator('#tui-toast')).toContainText('no new tracks');
  await expect(page.locator('#tui-queue-count')).toHaveText('14');
});

test('track controls are keyboard accessible and lyrics follow the selected track', async ({ page }) => {
  await openDemo(page);
  await page.locator('#tui-tab-1').click();
  const yellow = page.getByRole('button', { name: /Play Yellow by Coldplay/ });
  await yellow.focus();
  await page.keyboard.press('Space');
  await expect(page.locator('#tui-header-track')).toHaveText('Yellow — Coldplay');
  await expect(yellow).toBeFocused();
  await page.locator('#tui-btn-lyrics').click();
  await expect(page.locator('#tui-lyrics-heading')).toContainText('Yellow');
  await expect(page.locator('#tui-lyrics-content')).toContainText('Look at the stars');
  await expect(page.locator('#tui-lyrics-content')).not.toContainText('real life');
});

test('shortcuts are scoped to the demo and do not intercept native buttons', async ({ page }) => {
  await page.goto('/docs/index.html');
  const installTab = page.locator('#install-tab-1');
  await installTab.focus();
  await page.keyboard.press('Space');
  await expect(page.locator('#install-content-1')).toBeVisible();
  await expect(page.locator('#tui-play-btn')).toHaveAttribute('aria-label', 'Pause demo playback');

  await page.locator('#demo-disclosure summary').click();
  const screen = page.locator('#tui-terminal-screen');
  await screen.focus();
  await page.keyboard.press('Space');
  await expect(page.locator('#tui-play-btn')).toHaveAttribute('aria-label', 'Play demo playback');
  await page.keyboard.press('t');
  await expect(page.locator('#tui-theme-label')).toHaveText('Amber');
  await page.locator('#tui-shortcuts-toggle').uncheck();
  await page.keyboard.press('t');
  await expect(page.locator('#tui-theme-label')).toHaveText('Amber');
});

test('reduced motion and offscreen state pause decorative animation', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await openDemo(page);
  const bar = page.locator('#tui-visualizer-view .bar-anim').first();
  await page.locator('#tui-btn-vis').click();
  await expect(bar).toBeVisible();
  await expect.poll(() => bar.evaluate((element) => parseFloat(getComputedStyle(element).animationDuration))).toBeLessThan(0.01);
  await page.locator('footer').scrollIntoViewIfNeeded();
  await expect(page.locator('#tui-terminal-screen')).toHaveClass(/demo-offscreen/);
});

test('mobile layout stays within the viewport', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/docs/index.html');
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
  await expect.poll(() => page.locator('#interactive-demo').evaluate((element) => element.scrollWidth <= element.clientWidth + 1)).toBe(true);
  await page.locator('#demo-disclosure summary').click();
  for (const id of ['tui-radio-btn', 'tui-tab-5', 'tui-btn-lyrics']) {
    const bounds = await page.locator(`#${id}`).boundingBox();
    expect(bounds.x).toBeGreaterThanOrEqual(0);
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(390);
  }
});

test('tabs support arrow navigation and ranges retain native keyboard behavior', async ({ page }) => {
  await openDemo(page);
  await page.locator('#tui-tab-3').focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.locator('#tui-tab-4')).toBeFocused();
  await expect(page.locator('#tui-tab-4')).toHaveAttribute('aria-selected', 'true');
  const volume = page.locator('#tui-volume');
  const before = Number(await volume.inputValue());
  await volume.focus();
  await page.keyboard.press('ArrowLeft');
  expect(Number(await volume.inputValue())).toBeLessThan(before);
  await page.locator('#install-tab-0').focus();
  await page.keyboard.press('End');
  await expect(page.locator('#install-tab-2')).toBeFocused();
  await expect(page.locator('#install-content-2')).toBeVisible();
});

test('paused demo suspends animation and all themes have distinct accents', async ({ page }) => {
  await openDemo(page);
  const screen = page.locator('#tui-terminal-screen');
  const accents = [];
  await expect.poll(() => page.locator('.bar-anim').last().evaluate((element) => element.getBoundingClientRect().height)).toBeGreaterThan(0);
  await screen.focus();
  for (let index = 0; index < 6; index += 1) {
    accents.push(await screen.evaluate((element) => getComputedStyle(element).getPropertyValue('--term-accent').trim()));
    await page.keyboard.press('t');
  }
  expect(new Set(accents).size).toBe(6);
  await page.locator('#tui-btn-vis').click();
  await page.locator('#tui-play-btn').click();
  await expect.poll(() => page.locator('#tui-visualizer-view .bar-anim').first().evaluate((element) => getComputedStyle(element).animationPlayState)).toBe('paused');
});

test('copy fallback works when the clipboard API is absent', async ({ page }) => {
  await page.goto('/docs/index.html');
  await page.locator('#install-tab-1').click();
  await page.evaluate(() => Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true }));
  await page.locator('#install-content-1 button').click();
  await expect(page.locator('#install-content-1 button')).toContainText('Select command');
});

test('release downloads and install commands match the Rust and website versions', async ({ page }) => {
  const fs = require('node:fs');
  const path = require('node:path');
  const root = path.resolve(__dirname, '..');
  const packageInfo = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  const lock = JSON.parse(fs.readFileSync(path.join(root, 'package-lock.json'), 'utf8'));
  const cargo = fs.readFileSync(path.join(root, 'Cargo.toml'), 'utf8');
  const version = cargo.match(/\[package\][\s\S]*?\bversion\s*=\s*"([^"]+)"/)[1];
  expect(packageInfo.version).toBe(version);
  expect(lock.version).toBe(version);
  expect(lock.packages[''].version).toBe(version);
  await page.goto('/docs/index.html');
  await expect(page.locator('.version-mark')).toHaveText(version);
  const urls = await page.locator('a[href*="/releases/download/"]').evaluateAll(links => links.map(link => link.href));
  expect(urls.length).toBeGreaterThanOrEqual(4);
  for (const url of urls) {
    expect(url).toMatch(new RegExp(`^https://github\\.com/braces157/tutify/releases/download/v${version.replaceAll('.', '\\.')}(/tuitify\\.exe|/Tuitify-${version.replaceAll('.', '\\.')}\\-windows-x86_64\\.zip)$`));
  }
  await page.locator('#install-tab-1').click();
  await expect(page.locator('#install-content-1 code')).toContainText(`--tag v${version}`);
  await expect(page.locator('a[href*="github.com/braces157/tuitify"]')).toHaveCount(0);
});

test('copy controls explain clipboard failures and leave command text selectable', async ({ page }) => {
  await page.goto('/docs/index.html');
  await page.locator('#install-tab-1').click();
  const copyButton = page.locator('#install-content-1 button');
  await page.evaluate(() => {
    navigator.clipboard.writeText = () => Promise.reject(new Error('permission denied'));
  });
  await page.evaluate(() => window.copyCommand('cargo install --git https://github.com/braces157/tutify.git', document.querySelector('#install-content-1 button')));
  await expect(copyButton).toContainText('Select command');
  await expect(page.locator('#install-content-1 code')).toHaveClass(/select-text/);
});

test('craft design motion preference persists and can follow the device', async ({page}) => {
  await page.goto('/docs/index.html');
  const toggle = page.locator('.motion-toggle');
  await expect(toggle).toHaveAttribute('aria-pressed','true');
  await page.emulateMedia({reducedMotion:'reduce'});
  await expect(toggle).toHaveAttribute('aria-pressed','false');
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-pressed','true');
  await page.reload();
  await expect(toggle).toHaveAttribute('aria-pressed','true');
  await toggle.click();
  await expect(page.locator('html')).toHaveClass(/motion-paused/);
  await page.reload();
  await expect(toggle).toHaveAttribute('aria-pressed','false');
});

test('terminal illustration animation pauses while offscreen and on request', async ({page}) => {
  await page.goto('/docs/index.html');
  const preview = page.locator('.terminal-showcase');
  await preview.scrollIntoViewIfNeeded();
  const bar = page.locator('.preview-bar').first();
  await expect(bar).toHaveCSS('animation-play-state','running');
  await page.locator('footer').scrollIntoViewIfNeeded();
  await expect(bar).toHaveCSS('animation-play-state','paused');
  await preview.scrollIntoViewIfNeeded();
  await page.locator('.motion-toggle').click();
  await expect(bar).toHaveCSS('animation-play-state','paused');
});

test('production design has no dead links, fake metrics, or mock alert controls', async ({page}) => {
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto('/docs/index.html');
  await expect(page.locator('a[href="#"], [onclick*="alert("]')).toHaveCount(0);
  const missing = await page.locator('a[href^="#"]').evaluateAll(links => links.map(a=>a.hash).filter(hash=>!document.getElementById(hash.slice(1))));
  expect(missing).toEqual([]);
  await expect(page.locator('body')).not.toContainText('14MB');
  await expect(page.locator('body')).not.toContainText('88ms');
  await expect(page.locator('body')).not.toContainText('Homebrew');
  expect(errors).toEqual([]);
});

test('hero copy uses the pinned release command and explains clipboard failure', async ({page}) => {
  await page.goto('/docs/index.html');
  await page.evaluate(() => Object.defineProperty(navigator,'clipboard',{value:undefined,configurable:true}));
  await page.locator('#hero-copy-cmd').click();
  await expect(page.locator('#install-content-1')).toBeVisible();
  await expect(page.locator('#install-content-1 code')).toContainText(`--tag v${releaseVersion}`);
  await expect(page.locator('#hero-copy-cmd')).toContainText('Select command');
});

test('mobile navigation supports opening, escape, and section links', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/docs/index.html');
  const menu = page.locator('.menu-toggle');
  await expect(page.locator('#mobile-nav')).toBeHidden();
  await menu.click();
  await expect(menu).toHaveAttribute('aria-expanded', 'true');
  await page.locator('#mobile-nav a').first().focus();
  await page.keyboard.press('Escape');
  await expect(menu).toBeFocused();
  await expect(page.locator('#mobile-nav')).toBeHidden();
  await menu.click();
  await page.locator('#mobile-nav a[href="#install"]').click();
  await expect(page).toHaveURL(/#install$/);
  await expect(menu).toHaveAttribute('aria-expanded', 'false');
});

test('keycap controls operate the actual browser demo', async ({ page }) => {
  await page.goto('/docs/index.html');
  await page.locator('[data-demo-action="next"]').click();
  await expect(page.locator('#tui-header-track')).toHaveText('Yellow — Coldplay');
  await expect(page.locator('#tui-terminal-screen')).toBeFocused();
  await page.locator('[data-demo-action="theme"]').click();
  await expect(page.locator('#tui-theme-label')).toHaveText('Amber');
  await page.locator('[data-demo-action="lyrics"]').click();
  await expect(page.locator('#tui-lyrics-heading')).toContainText('Yellow');
  await page.locator('[data-demo-action="search"]').click();
  await expect(page.locator('#tui-filter-input')).toBeFocused();
  await page.locator('#tui-filter-input').fill('bowie');
  await expect(page.locator('#tui-rows-container .tui-track-row')).toHaveCount(1);
});

test('animated layout fits phones, tablets, and desktop screens', async ({ page }) => {
  await page.goto('/docs/index.html');
  await page.evaluate(() => document.fonts.ready);
  for (const width of [320, 390, 768, 1024, 1440, 1920]) {
    await page.setViewportSize({ width, height: 900 });
    await page.waitForTimeout(250);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), `overflow at ${width}px`).toBe(true);
    for (const id of ['features', 'interactive-demo', 'install']) {
      expect(await page.locator(`#${id}`).evaluate(element => element.scrollWidth <= element.clientWidth + 1), `${id} overflow at ${width}px`).toBe(true);
      await page.locator(`#${id}`).scrollIntoViewIfNeeded();
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), `scrolling to ${id} at ${width}px`).toBe(true);
    }
    await page.evaluate(() => window.scrollTo(0, 0));
  }
});

test('essential content and downloads work without JavaScript', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto('http://127.0.0.1:4173/docs/index.html');
  await expect(page.locator('h1')).toBeVisible();
  await expect(page.locator('.button-primary').first()).toHaveAttribute('href', `https://github.com/braces157/tutify/releases/download/v${releaseVersion}/tuitify.exe`);
  await expect(page.locator('.install-copy')).toBeVisible();
  await expect(page.locator('.questions-heading')).toHaveCSS('opacity', '1');
  await context.close();
});

test('native screenshot previews switch with keyboard navigation and keep full-image links', async ({ page }) => {
  await page.goto('/docs/index.html');
  const queue = page.locator('#preview-tab-queue');
  await queue.focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.locator('#preview-tab-lyrics')).toBeFocused();
  await expect(page.locator('#preview-tab-lyrics')).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('#preview-panel')).toHaveAttribute('aria-labelledby', 'preview-tab-lyrics');
  await expect(page.locator('#preview-panel img')).toHaveAttribute('src', /v0\.6\.0\/glass-lyrics-readings-volta\.png$/);
  await expect(page.locator('#preview-panel a')).toHaveAttribute('href', /v0\.6\.0\/glass-lyrics-readings-volta\.png$/);
  await expect(page.locator('.preview-caption')).toContainText('v0.6.0');
  await expect.poll(() => page.locator('#preview-panel img').evaluate(image => image.complete && image.naturalWidth === 2854 && image.naturalHeight === 1722)).toBe(true);
  await page.keyboard.press('End');
  await expect(page.locator('#preview-tab-spectrum')).toBeFocused();
  await expect(page.locator('#preview-panel img')).toHaveAttribute('alt', /spectrum visualizer/);
  await page.keyboard.press('Home');
  await expect(queue).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('#preview-panel a')).toHaveAttribute('href', /v0\.7\.0\/demo-queue-wide\.png$/);
});

test('native preview explains image failures and recovers on another selection', async ({ page }) => {
  await page.route('**/glass-lyrics-readings-volta.png', route => route.abort());
  await page.goto('/docs/index.html');
  await page.locator('#preview-tab-lyrics').click();
  await expect(page.locator('.preview-error')).toBeVisible();
  await expect(page.locator('#preview-panel a')).toHaveAttribute('href', /glass-lyrics-readings-volta\.png$/);
  await page.locator('#preview-tab-spectrum').click();
  await expect(page.locator('.preview-error')).toBeHidden();
  await expect.poll(() => page.locator('#preview-panel img').evaluate(image => image.complete && image.naturalWidth > 0)).toBe(true);
});
