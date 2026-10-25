const { test, expect } = require('@playwright/test');

async function openPlayer(page) {
  await page.goto('/player.html');
  await expect(page.locator('.tui-track-row')).toHaveCount(5);
}

test('player shares the homepage theme and loads without errors or remote assets', async ({ page }) => {
  const errors = [];
  const remoteAssets = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => {
    if (!request.isNavigationRequest() && new URL(request.url()).origin !== 'http://127.0.0.1:4173') remoteAssets.push(request.url());
  });
  await openPlayer(page);
  await expect(page).toHaveTitle('Tuitify — Try the player');
  await expect(page.locator('.site-header [aria-current="page"]')).toHaveText('Player');
  await expect(page.locator('#tui-terminal-screen')).toHaveCSS('--term-accent', '#d6ff5e');
  await expect(page.locator('body')).toHaveCSS('background-color', 'rgb(9, 13, 12)');
  await expect(page.locator('#tui-demo-note')).toContainText('no audio');
  expect(errors).toEqual([]);
  expect(remoteAssets).toEqual([]);
});

test('views expose distinct content and support keyboard tab navigation', async ({ page }) => {
  await openPlayer(page);
  for (const [view, count] of [[1, 8], [2, 4], [3, 5], [4, 8]]) {
    await page.locator(`#tui-tab-${view}`).click();
    await expect(page.locator('.tui-track-row')).toHaveCount(count);
    await expect(page.locator(`#tui-tab-${view}`)).toHaveAttribute('aria-selected', 'true');
  }
  await page.locator('#tui-tab-4').focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.locator('#tui-tab-5')).toBeFocused();
  await expect(page.locator('#tui-help-view')).toBeVisible();
  await expect(page.locator('#tui-catalog-view')).toBeHidden();
  await page.keyboard.press('Home');
  await expect(page.locator('#tui-tab-1')).toBeFocused();
});

test('filter, empty state, selected song and current-row pause work together', async ({ page }) => {
  await openPlayer(page);
  const filter = page.locator('#tui-filter-input');
  await filter.fill('coldplay yellow');
  await expect(page.locator('.tui-track-row')).toHaveCount(1);
  await filter.press('Enter');
  await expect(page.locator('#tui-track-title')).toHaveText('Yellow');
  await expect(page.locator('#tui-track-artist')).toHaveText('Coldplay');
  await page.locator('#tui-seek').fill('40');
  await page.getByRole('button', { name: 'Pause Yellow by Coldplay', exact: true }).click();
  await expect(page.locator('#tui-play-btn')).toHaveAttribute('aria-pressed', 'false');
  await expect(page.locator('#tui-curr-time')).toHaveText('0:40');
  await expect(page.getByRole('button', { name: 'Play Yellow by Coldplay', exact: true })).toBeFocused();
  await expect(page.locator('#tui-play-btn svg')).toHaveCount(1);
  await filter.fill('no such song');
  await expect(page.locator('.track-empty')).toContainText('No songs found.');
  await filter.press('Escape');
  await expect(filter).toHaveValue('');
  await expect(page.locator('.tui-track-row')).toHaveCount(5);
});

test('visible transport, lyrics and native range controls stay in sync', async ({ page }) => {
  await openPlayer(page);
  await page.getByRole('button', { name: 'Next demo track', exact: true }).click();
  await expect(page.locator('#tui-track-title')).toHaveText('Yellow');
  await page.locator('#tui-btn-lyrics').click();
  await expect(page.locator('#tui-lyrics-heading')).toContainText('Yellow');
  await expect(page.locator('#tui-lyrics-content')).toContainText('Look at the stars');
  await page.locator('#tui-seek').fill('42');
  await expect(page.locator('.lyric-line-active')).toContainText('Yeah, they were all yellow');
  await page.locator('#tui-btn-vis').click();
  await expect(page.locator('#tui-visualizer-view')).toBeVisible();
  await expect(page.locator('#tui-lyrics-view')).toBeHidden();
  await expect(page.locator('#tui-btn-lyrics')).toHaveAttribute('aria-pressed', 'false');
  await page.getByRole('button', { name: 'Previous demo track', exact: true }).click();
  await expect(page.locator('#tui-track-title')).toHaveText('Bohemian Rhapsody');
  const volume = page.locator('#tui-volume');
  await volume.fill('55');
  await expect(page.locator('#tui-vol-label')).toHaveText('55%');
  await volume.focus();
  await page.keyboard.press('ArrowLeft');
  await expect(volume).toHaveValue('54');
  await expect(page.locator('#tui-vol-label')).toHaveText('54%');
});

test('radio adds related tracks once and reports the resulting queue', async ({ page }) => {
  await openPlayer(page);
  await page.locator('#tui-radio-btn').click();
  await expect(page.locator('#tui-toast')).toContainText('queued 6 related tracks');
  await expect(page.locator('#tui-queue-count')).toHaveText('14');
  await page.locator('#tui-tab-4').click();
  await expect(page.locator('.tui-track-row')).toHaveCount(14);
  await page.locator('#tui-radio-btn').click();
  await expect(page.locator('#tui-toast')).toContainText('no new tracks');
  await expect(page.locator('.tui-track-row')).toHaveCount(14);
});

test('shortcuts respect the toggle, native controls and hidden search views', async ({ page }) => {
  await openPlayer(page);
  const screen = page.locator('#tui-terminal-screen');
  await screen.focus();
  await page.keyboard.press('Space');
  await expect(page.locator('#tui-play-btn')).toHaveAttribute('aria-pressed', 'false');
  await page.keyboard.press('t');
  await expect(page.locator('#tui-theme-label')).toHaveText('Amber');
  await page.locator('#tui-shortcuts-toggle').uncheck();
  await screen.focus();
  await page.keyboard.press('Space');
  await page.keyboard.press('t');
  await expect(page.locator('#tui-play-btn')).toHaveAttribute('aria-pressed', 'false');
  await expect(page.locator('#tui-theme-label')).toHaveText('Amber');
  await page.locator('#tui-shortcuts-toggle').check();
  await page.locator('#tui-tab-5').click();
  await screen.focus();
  await page.keyboard.press('/');
  await expect(page.locator('#tui-filter-input')).toBeFocused();
  await expect(page.locator('#tui-tab-1')).toHaveAttribute('aria-selected', 'true');
  await page.locator('#tui-filter-input').fill('t');
  await expect(page.locator('#tui-theme-label')).toHaveText('Amber');
});

test('all six themes differ and decorative motion pauses with playback', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await openPlayer(page);
  const accents = [];
  for (let index = 0; index < 6; index++) {
    accents.push(await page.locator('#tui-terminal-screen').evaluate(el => getComputedStyle(el).getPropertyValue('--term-accent').trim()));
    await page.getByRole('button', { name: 'Cycle demo theme', exact: true }).click();
  }
  expect(new Set(accents).size).toBe(6);
  await page.locator('#tui-btn-vis').click();
  await expect(page.locator('.bar-anim').first()).toHaveCSS('animation-play-state', 'running');
  await page.locator('#tui-play-btn').click();
  await expect(page.locator('.bar-anim').first()).toHaveCSS('animation-play-state', 'paused');
  await expect(page.locator('.player-record')).toHaveCSS('animation-play-state', 'paused');
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await expect.poll(() => page.locator('.player-record').evaluate(el => parseFloat(getComputedStyle(el).animationDuration))).toBeLessThan(.01);
});

for (const width of [320, 360, 390, 650, 768, 1024, 1440, 1920]) {
  test(`player controls fit inside their container at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await openPlayer(page);
    const overflow = await page.evaluate(() => {
      const frame = document.querySelector('.player-window').getBoundingClientRect();
      const controls = [...document.querySelectorAll('.library-tabs button, .player-toolbar button, .transport-controls button, #tui-filter-input, #tui-seek, #tui-volume')];
      return controls.filter(el => {
        const rect = el.getBoundingClientRect();
        return rect.left < frame.left - 1 || rect.right > frame.right + 1;
      }).map(el => el.id || el.className).concat(
        [...document.querySelectorAll('.tui-tab')].filter(el => el.scrollWidth > el.clientWidth + 1).map(el => `${el.id}: clipped text`)
      );
    });
    expect(overflow).toEqual([]);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.locator('#tui-tab-5').click();
    await expect(page.locator('#tui-help-view')).toBeVisible();
  });
}

test('player keeps a useful native download fallback without JavaScript', async ({ browser }) => {
  const context = await browser.newContext({ javaScriptEnabled: false });
  const page = await context.newPage();
  await page.goto('http://127.0.0.1:4173/player.html');
  const download = page.getByRole('link', { name: 'download the native player', exact: true });
  await expect(download).toBeVisible();
  await expect(download).toHaveAttribute('href', 'index.html#install');
  await context.close();
});
