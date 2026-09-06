const { test, expect } = require('@playwright/test');

async function openDemo(page) {
  await page.goto('/docs/index.html');
  await page.locator('#interactive-demo').scrollIntoViewIfNeeded();
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
  await expect(page.locator('link[href="assets/tailwind.compiled.css"]')).toHaveCount(1);
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

  const screen = page.locator('#tui-terminal-screen');
  await screen.focus();
  await page.keyboard.press('Space');
  await expect(page.locator('#tui-play-btn')).toHaveAttribute('aria-label', 'Play demo playback');
  await page.keyboard.press('t');
  await expect(page.locator('#tui-theme-label')).toHaveText('Phosphor Green CRT');
  await page.locator('#tui-shortcuts-toggle').uncheck();
  await page.keyboard.press('t');
  await expect(page.locator('#tui-theme-label')).toHaveText('Phosphor Green CRT');
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
  for (let index = 0; index < 5; index += 1) {
    accents.push(await screen.evaluate((element) => getComputedStyle(element).getPropertyValue('--term-accent').trim()));
    await page.keyboard.press('t');
  }
  expect(new Set(accents).size).toBe(5);
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
