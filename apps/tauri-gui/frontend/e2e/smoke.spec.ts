import { expect, test, type Page } from '@playwright/test';

type TargetedApplyRequest = {
  requestId: string;
  path: string;
  target?: string;
};

type MockControl = {
  injectCommandFailure(command: string): void;
  clearCommandFailure(command: string): void;
  setBrowserFixtureCopies(copies: number): void;
  rejectNextBrowserAppend(message?: string): void;
  emptyNextBrowserAppend(): void;
  browserAppendRequestCount(): number;
  setScanProgress(progress: Record<string, unknown>): void;
  setSourceAvailability(id: number, availability: 'unknown' | 'available' | 'offline'): void;
  holdSourceRefresh(): void;
  releaseSourceRefresh(): void;
  sourceRefreshCallCount(): number;
  setFirstRunSourceSuggestions(suggestions: Array<
    | { kind: 'directory'; label: string; path: string }
    | { kind: 'wallpaperEngine'; roots: string[] }
  >): void;
  setRuntimeWallpaperObservations(observations: Array<{
    output: string;
    wallpaperPath: string | null;
    status: 'confirmed' | 'unknown';
    reason?: string;
  }>): void;
  setThumbnailFailure(path: string): void;
  lastTargetedApplyRequest(): TargetedApplyRequest | null;
};

declare global {
  interface Window {
    __mockControl?: MockControl;
  }
}

const pageErrors = new WeakMap<Page, string[]>();

test.beforeEach(({ page }) => {
  const errors: string[] = [];
  pageErrors.set(page, errors);
  page.on('pageerror', (error) => errors.push(error.message));
});

test.afterEach(({ page }) => {
  expect(pageErrors.get(page) ?? [], 'uncaught browser page errors').toEqual([]);
});

async function openApp(page: Page): Promise<void> {
  await page.goto('/');
  await expect(page.locator('.single-page-shell')).toBeVisible();
}

async function waitForGrid(page: Page): Promise<void> {
  await expect(page.locator('.wallpaper-grid')).toBeVisible();
  await expect(page.locator('.wallpaper-card').first()).toBeVisible();
}

async function lastApplyRequest(page: Page): Promise<TargetedApplyRequest | null> {
  return page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    return control.lastTargetedApplyRequest();
  });
}

async function browserAppendRequestCount(page: Page): Promise<number> {
  return page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    return control.browserAppendRequestCount();
  });
}

async function sourceRefreshCallCount(page: Page): Promise<number> {
  return page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    return control.sourceRefreshCallCount();
  });
}

async function scrollLibraryGrid(page: Page, edge: 'start' | 'end'): Promise<void> {
  await page.locator('.wallpaper-grid').evaluate((element, requestedEdge) => {
    element.scrollTop = requestedEdge === 'end' ? element.scrollHeight : 0;
    element.dispatchEvent(new Event('scroll'));
  }, edge);
}

async function openSettings(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Open settings' }).click();
  await expect(page.getByRole('dialog', { name: 'Settings' })).toBeVisible();
}

async function openSources(page: Page): Promise<void> {
  await openSettings(page);
  await page.getByRole('button', { name: 'Manage wallpaper sources' }).click();
  await expect(page.getByRole('dialog', { name: 'Wallpaper sources' })).toBeVisible();
}

function sourceRow(page: Page, sourceId: number) {
  return page.locator(`[data-source-id="${sourceId}"]`);
}

async function removeSource(page: Page, sourceId: number): Promise<void> {
  const row = sourceRow(page, sourceId);
  await row.getByRole('button', { name: /^Remove / }).click();
  const confirmation = row.getByRole('alertdialog');
  await expect(confirmation).toBeVisible();
  await confirmation.getByRole('button', { name: 'Remove source' }).click();
  await expect(row).toHaveCount(0);
}

test('renders one unified wallpaper picker without legacy navigation', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);

  await expect(page.getByLabel('Wallpaper Console')).toBeAttached();
  await expect(page.getByLabel('Search wallpapers')).toBeVisible();
  await expect(page.getByLabel('Library filters')).toBeVisible();
  await expect(page.getByLabel('Source filter')).toBeVisible();
  await expect(page.getByLabel('Wallpaper type filter')).toBeVisible();
  await expect(page.getByLabel('Library sort')).toBeVisible();
  await expect(page.getByLabel('Card size')).toBeVisible();
  await expect(page.getByLabel('Display target')).toBeVisible();

  await expect(page.getByLabel('Source filter')).toHaveCSS('padding-top', '0px');
  await expect(page.getByLabel('Source filter')).toHaveCSS('padding-bottom', '0px');
  await expect(page.getByLabel('Card size').locator('option')).toHaveText([
    'Small',
    'Medium',
    'Large',
  ]);

  await expect(page.locator('.sidebar')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'History', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Favorites', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Sources', exact: true })).toHaveCount(0);
});

test('runtime reconciliation confirms current state and clears it after renderer exit', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await expect(page.locator('.single-page-statusbar__current')).toHaveText('Current: wallpaper-001.jpg');

  await page.evaluate(() => window.__mockControl?.setRuntimeWallpaperObservations([
    { output: 'eDP-1', wallpaperPath: null, status: 'unknown', reason: 'renderer stopped' },
    { output: 'HDMI-A-1', wallpaperPath: null, status: 'unknown', reason: 'renderer stopped' },
  ]));

  await expect(page.locator('.single-page-statusbar__current')).toHaveText(
    'Current: not verified',
    { timeout: 8_000 },
  );
  await expect(page.locator('.wallpaper-card.current')).toHaveCount(0);
});

test('search, source, type, favorites, and sort compose in the unified grid', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);

  await page.getByLabel('Source filter').selectOption('source:1');
  await page.getByLabel('Wallpaper type filter').selectOption('image');
  await page.getByLabel('Library sort').selectOption('nameDesc');

  await expect(page.locator('.wallpaper-card').first()).toHaveAttribute(
    'title',
    'wallpaper-148.jpg · Pictures',
  );
  await expect(page.locator('.wallpaper-card').first()).toHaveAttribute(
    'data-wallpaper-path',
    '/mock/path/wallpaper-148.jpg',
  );
  const filteredPaths = await page.locator('.wallpaper-card').evaluateAll((cards) =>
    cards.map((card) => card.getAttribute('data-wallpaper-path') ?? ''),
  );
  expect(filteredPaths.length).toBeGreaterThan(0);
  expect(filteredPaths.every((path) => /wallpaper-\d*[02468]\.jpg$/.test(path))).toBe(true);

  await page.getByLabel('Search wallpapers').fill('wallpaper-002');
  await expect(page.locator('.single-page-count')).toHaveText('1 / 1');
  await expect(page.locator('.wallpaper-card')).toHaveCount(1);
  await expect(page.locator('.wallpaper-card')).toHaveAttribute(
    'data-wallpaper-path',
    '/mock/path/wallpaper-002.jpg',
  );

  await page.getByLabel('Search wallpapers').fill('');
  await page.getByLabel('Source filter').selectOption('all');
  await page.getByLabel('Favorites').check();
  await expect(page.locator('.single-page-count')).toHaveText('1 / 1');
  await expect(page.locator('.wallpaper-card')).toHaveAttribute(
    'data-wallpaper-path',
    '/mock/path/wallpaper-001.jpg',
  );
});

test('card heart adds and removes a favorite without applying the wallpaper', async ({ page }) => {
  await openApp(page);
  await page.getByLabel('Wallpaper type filter').selectOption('image');

  const card = page.locator('[data-wallpaper-path="/mock/path/wallpaper-002.jpg"]');
  await expect(card).toBeVisible();
  const addFavorite = card.getByRole('button', { name: 'Add favorite' });
  await expect(addFavorite).toHaveCSS('opacity', '0');

  await card.hover();
  await expect(addFavorite).toHaveCSS('opacity', '1');
  await expect(addFavorite).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  const applyBeforeFavorite = await lastApplyRequest(page);
  await addFavorite.click();

  const removeFavorite = card.getByRole('button', { name: 'Remove favorite' });
  await expect(removeFavorite).toBeVisible();
  await expect(removeFavorite).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  expect(await lastApplyRequest(page)).toEqual(applyBeforeFavorite);

  await removeFavorite.click();
  await expect(card.getByRole('button', { name: 'Add favorite' })).toBeAttached();
  expect(await lastApplyRequest(page)).toEqual(applyBeforeFavorite);
});

test('random selection obeys every active filter', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);

  await page.getByLabel('Source filter').selectOption('source:1');
  await page.getByLabel('Wallpaper type filter').selectOption('video');
  await page.getByLabel('Search wallpapers').fill('wallpaper-020');
  await expect(page.locator('.single-page-count')).toHaveText('1 / 1');

  await page.getByRole('button', { name: 'Apply a random wallpaper from active filters' }).click();
  await expect.poll(() => lastApplyRequest(page)).toMatchObject({
    path: '/mock/path/wallpaper-020.mp4',
  });
  await expect(page.locator('.single-page-statusbar__selection')).toContainText('wallpaper-020.mp4');
});

test('single/double-click setting and display target govern apply requests', async ({ page }) => {
  await openApp(page);
  await page.getByLabel('Wallpaper type filter').selectOption('image');
  await waitForGrid(page);

  const firstCard = page.locator('.wallpaper-card').nth(0);
  const firstPath = await firstCard.getAttribute('data-wallpaper-path');
  expect(firstPath).toBeTruthy();
  await firstCard.click();
  await expect.poll(() => lastApplyRequest(page)).toMatchObject({ path: firstPath! });

  await openSettings(page);
  await page.getByLabel('Apply gesture').selectOption('double');
  await page.getByRole('button', { name: 'Close settings' }).click();
  await page.getByLabel('Display target').selectOption({ label: 'HDMI-A-1' });

  const secondCard = page.locator('.wallpaper-card').nth(1);
  const secondPath = await secondCard.getAttribute('data-wallpaper-path');
  expect(secondPath).toBeTruthy();
  const firstRequest = await lastApplyRequest(page);
  await secondCard.click();
  await expect(page.locator('.single-page-statusbar__selection')).toContainText(
    secondPath!.split('/').at(-1)!,
  );
  await page.waitForTimeout(250);
  expect(await lastApplyRequest(page)).toEqual(firstRequest);

  await secondCard.dblclick();
  await expect.poll(() => lastApplyRequest(page)).toMatchObject({
    path: secondPath!,
    target: 'HDMI-A-1',
  });
});

test('compact settings contains exactly the three user-facing groups', async ({ page }) => {
  await openApp(page);
  const grid = page.locator('.wallpaper-grid');
  await expect(grid).toHaveCSS('overflow-y', 'auto');
  await openSettings(page);
  await expect(grid).toHaveCSS('overflow-y', 'hidden');

  const dialog = page.getByRole('dialog', { name: 'Settings' });
  await expect(dialog.locator('[data-settings-group]')).toHaveCount(3);
  await expect(dialog.locator('[data-behavior-card]')).toHaveCount(4);
  await expect(dialog.getByRole('heading', { name: 'Interface', exact: true })).toBeVisible();
  await expect(dialog.getByRole('heading', { name: 'Renderer selection' })).toBeVisible();
  await expect(dialog.getByRole('heading', { name: 'Fill & transition' })).toBeVisible();
  await expect(dialog.getByRole('heading', { name: 'Scene playback' })).toBeVisible();
  await expect(dialog.getByRole('heading', { name: 'Restore', exact: true })).toBeVisible();
  await expect(dialog.getByRole('heading', { name: 'Appearance & interaction' })).toBeVisible();
  await expect(dialog.getByRole('heading', { name: 'Wallpaper behavior' })).toBeVisible();
  await expect(dialog.getByRole('heading', { name: 'Sources', exact: true })).toBeVisible();
  await expect(dialog.getByLabel('Theme')).toHaveValue('system');
  await expect(dialog.getByLabel('Apply gesture')).toHaveValue('single');
  await expect(dialog.getByRole('group', { name: 'Image' }).getByRole('button', { name: 'awww' }))
    .toHaveAttribute('aria-pressed', 'true');
  await expect(dialog.getByRole('group', { name: 'GIF' }).getByRole('button', { name: 'awww' }))
    .toHaveAttribute('aria-pressed', 'true');
  await expect(dialog.getByRole('group', { name: 'Video' }).getByRole('button', { name: 'mpvpaper' }))
    .toHaveAttribute('aria-pressed', 'true');
  await expect(dialog.getByText('Default display', { exact: true })).toHaveCount(0);
  await expect(dialog.getByLabel('Renderer installation status')).toHaveCount(0);
  await expect(dialog.getByLabel('Wallpaper Engine scaling')).toBeDisabled();
  await expect(dialog.getByRole('button', { name: 'Manage wallpaper sources' })).toBeVisible();

  for (const forbidden of [
    'Database',
    'Cache TTL',
    'Raw config',
    'Runtime stages',
    'Repair Database',
  ]) {
    await expect(dialog.getByText(forbidden, { exact: true })).toHaveCount(0);
  }

  await dialog.getByRole('button', { name: 'Close settings' }).click();
  await expect(grid).toHaveCSS('overflow-y', 'auto');
});

test('source drawer adds, renames, configures, refreshes, and safely removes sources', async ({ page }) => {
  await openApp(page);
  await openSources(page);

  const configuredSources = page.getByRole('list', { name: 'Configured wallpaper sources' });
  await expect(configuredSources.locator('[data-source-id]')).toHaveCount(3);
  await expect(configuredSources.locator('[data-source-availability="available"]')).toHaveCount(3);

  await page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    control.setSourceAvailability(2, 'offline');
  });

  await page.getByRole('dialog', { name: 'Wallpaper sources' })
    .getByRole('button', { name: 'Add folder', exact: true })
    .click();
  await expect(configuredSources.locator('[data-source-id]')).toHaveCount(4);
  await expect(sourceRow(page, 4)).toContainText('/mock/selected/dir');
  await expect(sourceRow(page, 2)).toHaveAttribute('data-source-id', '2');
  await expect(sourceRow(page, 2).locator('[data-source-availability="offline"]')).toContainText(
    'Offline — indexed wallpapers are kept',
  );

  await page.getByRole('button', { name: 'Close wallpaper sources' }).click();
  await openSettings(page);
  const settings = page.getByRole('dialog', { name: 'Settings' });
  await expect(settings.getByText('4 sources · 1 offline')).toBeVisible();
  await settings.getByRole('button', { name: 'Manage wallpaper sources' }).click();

  const pictures = sourceRow(page, 1);
  await pictures.getByRole('button', { name: 'Rename Pictures' }).click();
  let alias = pictures.getByLabel('Alias for Pictures');
  await alias.fill('Discard this alias');
  await pictures.getByText('/mock/Pictures', { exact: true }).click();
  await expect(alias).toHaveCount(0);
  await expect(pictures).toContainText('Pictures');

  await pictures.getByRole('button', { name: 'Rename Pictures' }).click();
  alias = pictures.getByLabel('Alias for Pictures');
  await alias.fill('Art collection');
  await alias.press('Enter');
  await expect(sourceRow(page, 1)).toContainText('Art collection');

  const recursive = sourceRow(page, 1).getByRole('switch', {
    name: 'Scan Art collection recursively',
  });
  await expect(recursive).toBeChecked();
  await recursive.uncheck();
  await expect(recursive).not.toBeChecked();

  await page.getByRole('button', { name: 'Refresh all' }).click();
  await expect(page.locator('[data-feedback-card="scan"]')).toContainText('All sources refreshed');

  await sourceRow(page, 1).getByRole('button', { name: 'Remove Art collection' }).click();
  const confirmation = sourceRow(page, 1).getByRole('alertdialog');
  await expect(confirmation).toContainText('It does not delete wallpaper files.');
  await confirmation.getByRole('button', { name: 'Cancel' }).click();
  await expect(sourceRow(page, 1)).toBeVisible();

  await removeSource(page, 1);
  await expect(configuredSources.locator('[data-source-id]')).toHaveCount(3);
});

test('source refresh remains busy across close and reopen without a duplicate request', async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    control.holdSourceRefresh();
  });
  await openSources(page);

  const dialog = page.getByRole('dialog', { name: 'Wallpaper sources' });
  await sourceRow(page, 1).getByRole('button', { name: 'Refresh Pictures' }).click();
  await expect(dialog).toHaveAttribute('aria-busy', 'true');
  await expect(dialog.getByRole('status').filter({ hasText: 'Refreshing source' })).toBeVisible();
  await expect.poll(() => dialog.locator('[data-source-mutating="true"]')
    .evaluateAll((elements) => elements.length > 0 && elements.every(
      (element) => (element as HTMLButtonElement | HTMLInputElement).disabled,
    ))).toBe(true);
  await expect.poll(() => sourceRefreshCallCount(page)).toBe(1);

  await dialog.getByRole('button', { name: 'Close wallpaper sources' }).click();
  await expect(dialog).toHaveCount(0);
  await openSources(page);

  const reopened = page.getByRole('dialog', { name: 'Wallpaper sources' });
  await expect(reopened).toHaveAttribute('aria-busy', 'true');
  await expect(reopened.getByRole('status').filter({ hasText: 'Refreshing source' })).toBeVisible();
  await expect.poll(() => reopened.locator('[data-source-mutating="true"]')
    .evaluateAll((elements) => elements.length > 0 && elements.every(
      (element) => (element as HTMLButtonElement | HTMLInputElement).disabled,
    ))).toBe(true);
  await page.waitForTimeout(300);
  expect(await sourceRefreshCallCount(page)).toBe(1);

  await page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    control.releaseSourceRefresh();
  });
  await expect(reopened).toHaveAttribute('aria-busy', 'false');
  await expect(reopened.getByRole('status').filter({ hasText: 'Refreshing source' })).toHaveCount(0);
  await expect.poll(() => reopened.locator('[data-source-mutating="true"]')
    .evaluateAll((elements) => elements.length > 0 && elements.every(
      (element) => !(element as HTMLButtonElement | HTMLInputElement).disabled,
    ))).toBe(true);
  await expect(page.locator('[data-feedback-card="scan"]')).toContainText('Source refresh finished');
  expect(await sourceRefreshCallCount(page)).toBe(1);
});

test('unsupported wallpaper context offers only browsing and limitation actions', async ({ page }) => {
  await openApp(page);
  await page.getByLabel('Wallpaper type filter').selectOption('unsupported');
  await expect(page.locator('.single-page-count')).toHaveText('2 / 2');

  const webWallpaper = page.locator('.wallpaper-card').filter({ hasText: 'Web title' });
  await webWallpaper.click({ button: 'right' });
  const menu = page.locator('.context-menu');
  await expect(menu.getByRole('button', { name: 'Add to Favorites' })).toBeVisible();
  await expect(menu.getByRole('button', { name: 'Open Location' })).toBeVisible();
  await expect(menu.getByRole('button', { name: 'Information' })).toBeVisible();
  await expect(menu.getByRole('button', { name: 'Limitation Details' })).toBeVisible();
  await expect(menu.getByRole('button', { name: 'Apply', exact: true })).toHaveCount(0);
  await expect(menu.getByText(/Retry|preview|linux-wallpaperengine/i)).toHaveCount(0);

  await menu.getByRole('button', { name: 'Limitation Details' }).click();
  await expect(page.locator('[data-feedback-card="system"]')).toContainText(
    'This wallpaper has a renderer limitation.',
  );
  await expect(page.locator('[data-feedback-progress="system"]')).toBeVisible();
});

test('feedback countdown pauses on hover and resumes to automatic dismissal', async ({ page }) => {
  await openApp(page);
  await page.getByLabel('Wallpaper type filter').selectOption('image');
  await page.getByLabel('Library sort').selectOption('nameDesc');
  const card = page.locator('.wallpaper-card').first();
  await expect(card).toBeVisible();
  await card.click({ button: 'right' });
  await page.locator('.context-menu').getByRole('button', { name: 'Add to Favorites' }).click();

  const feedback = page.locator('[data-feedback-card="system"]');
  const progress = feedback.getByRole('progressbar');
  await expect(feedback).toContainText('Added to favorites.');
  await expect(progress).toBeVisible();

  await feedback.hover();
  await page.waitForTimeout(150);
  const pausedAt = Number(await progress.getAttribute('aria-valuenow'));
  await page.waitForTimeout(650);
  expect(Number(await progress.getAttribute('aria-valuenow'))).toBe(pausedAt);

  await page.mouse.move(0, 0);
  await page.waitForTimeout(450);
  expect(Number(await progress.getAttribute('aria-valuenow'))).toBeLessThan(pausedAt);
  await expect(feedback).toHaveCount(0, { timeout: 3_500 });
});

test('scan activity is delayed, non-modal, and cancellable', async ({ page }) => {
  await openApp(page);
  await openSources(page);
  await page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    control.setScanProgress({
      running: true,
      stage: 'walking files',
      scanned: 5,
      totalHint: 100,
    });
  });

  await page.getByRole('button', { name: 'Refresh all' }).click();
  await page.getByRole('button', { name: 'Close wallpaper sources' }).click();
  const activity = page.locator('.scan-activity');
  await page.waitForTimeout(250);
  await expect(activity).toHaveCount(0);
  await expect(activity).toBeVisible({ timeout: 1_000 });
  await expect(activity).toHaveAttribute('data-non-modal', 'true');
  await expect(page.locator('.wallpaper-card').first()).toBeVisible();

  await activity.getByRole('button', { name: 'Cancel' }).click();
  await expect(activity).toHaveCount(0, { timeout: 1_000 });
});

test('1000+ entry fixture stays virtualized while scrolling, searching, filtering, and loading thumbnails', async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    control.setBrowserFixtureCopies(8);
    control.setThumbnailFailure('/mock/path/wallpaper-001.jpg');
    for (let copy = 1; copy < 8; copy += 1) {
      control.setThumbnailFailure(`/mock/browser-fixture-${copy}/path/wallpaper-001.jpg`);
    }
  });
  await page.getByLabel('Library sort').selectOption('nameDesc');
  await expect(page.locator('.single-page-count')).toHaveText('120 / 1216');

  for (let pageIndex = 2; pageIndex <= 9; pageIndex += 1) {
    await page.getByRole('button', { name: /Load more/ }).click();
    await expect(page.locator('.single-page-count')).toHaveText(`${pageIndex * 120} / 1216`);
  }
  expect(await page.locator('.wallpaper-card').count()).toBeLessThan(80);

  const grid = page.locator('.wallpaper-grid');
  const beforeScroll = await page.locator('.wallpaper-card').first().getAttribute('data-wallpaper-path');
  await grid.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
    element.dispatchEvent(new Event('scroll'));
  });
  await expect.poll(async () => page.locator('.wallpaper-card').first().getAttribute('data-wallpaper-path'))
    .not.toBe(beforeScroll);
  expect(await page.locator('.wallpaper-card').count()).toBeLessThan(80);

  await page.getByLabel('Wallpaper type filter').selectOption('image');
  await page.getByLabel('Search wallpapers').fill('wallpaper-001');
  await expect(page.locator('.single-page-count')).toHaveText('8 / 8');
  await expect(page.locator('.wallpaper-card').first().getByText('Preview failed')).toBeVisible();
});

test('5000+ entries auto-page near the virtual tail without expanding the DOM', async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    control.setBrowserFixtureCopies(33);
  });
  await page.getByLabel('Library sort').selectOption('nameDesc');
  await expect(page.locator('.single-page-count')).toHaveText('120 / 5016');
  expect(await page.locator('.wallpaper-card').count()).toBeLessThan(80);

  const grid = page.locator('.wallpaper-grid');
  await grid.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
    element.dispatchEvent(new Event('scroll'));
  });
  await expect(page.locator('.single-page-count')).toHaveText('240 / 5016');
  expect(await page.locator('.wallpaper-card').count()).toBeLessThan(80);

  await page.getByLabel('Search wallpapers').fill('wallpaper-001');
  await expect(page.locator('.single-page-count')).toHaveText('33 / 33');
  expect(await page.locator('.wallpaper-card').count()).toBeLessThan(40);
});

test('automatic paging fuse stops after one rejected append and manual retry recovers', async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    control.setBrowserFixtureCopies(33);
    control.rejectNextBrowserAppend('mock append transport failure');
  });
  await page.getByLabel('Library sort').selectOption('nameDesc');
  await expect(page.locator('.single-page-count')).toHaveText('120 / 5016');

  await scrollLibraryGrid(page, 'end');
  const retry = page.getByRole('button', { name: 'Retry loading more' });
  await expect(retry).toBeVisible();
  await expect(page.getByRole('alert')).toContainText('mock append transport failure');
  await expect.poll(() => browserAppendRequestCount(page)).toBe(1);

  for (let attempt = 0; attempt < 3; attempt += 1) {
    await scrollLibraryGrid(page, 'end');
  }
  await page.waitForTimeout(500);
  expect(await browserAppendRequestCount(page)).toBe(1);

  await scrollLibraryGrid(page, 'start');
  await retry.click();
  await expect(page.locator('.single-page-count')).toHaveText('240 / 5016');
  await expect.poll(() => browserAppendRequestCount(page)).toBe(2);
  await expect(page.getByRole('button', { name: 'Retry loading more' })).toHaveCount(0);
});

test('automatic paging fuse stops after one empty append and manual retry recovers', async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    control.setBrowserFixtureCopies(33);
    control.emptyNextBrowserAppend();
  });
  await page.getByLabel('Library sort').selectOption('nameDesc');
  await expect(page.locator('.single-page-count')).toHaveText('120 / 5016');

  await scrollLibraryGrid(page, 'end');
  const retry = page.getByRole('button', { name: 'Retry loading more' });
  await expect(retry).toBeVisible();
  await expect(page.locator('.single-page-count')).toHaveText('120 / 5016');
  await expect.poll(() => browserAppendRequestCount(page)).toBe(1);

  for (let attempt = 0; attempt < 3; attempt += 1) {
    await scrollLibraryGrid(page, 'end');
  }
  await page.waitForTimeout(500);
  expect(await browserAppendRequestCount(page)).toBe(1);

  await scrollLibraryGrid(page, 'start');
  await retry.click();
  await expect(page.locator('.single-page-count')).toHaveText('240 / 5016');
  await expect.poll(() => browserAppendRequestCount(page)).toBe(2);
  await expect(page.getByRole('button', { name: 'Retry loading more' })).toHaveCount(0);
});

test('repair is offered only after integrity verification confirms a fault', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await expect(page.getByRole('button', { name: 'Repair library' })).toHaveCount(0);

  await page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    control.injectCommandFailure('sqliteVerify');
    control.setBrowserFixtureCopies(0);
  });
  await page.getByLabel('Library sort').selectOption('nameAsc');

  const repair = page.getByRole('alert', { name: 'Library repair' });
  await expect(repair).toBeVisible();
  await expect(repair).toContainText('Library database needs repair');
  await repair.getByRole('button', { name: 'Repair library' }).click();
  await expect(repair).toHaveCount(0);
  await expect(page.getByText('Library index repaired')).toBeVisible();
});

test('an empty filtered view never triggers database repair verification', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    control.injectCommandFailure('sqliteVerify');
  });

  await page.getByLabel('Search wallpapers').fill('definitely-missing-wallpaper');
  await expect(page.getByText('No wallpapers match the active filters.')).toBeVisible();
  await expect(page.getByRole('alert', { name: 'Library repair' })).toHaveCount(0);
});

test('resizing from compact to wide keeps the visible scroll anchor mounted', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openApp(page);
  await waitForGrid(page);
  const grid = page.locator('.wallpaper-grid');
  await grid.evaluate((element) => {
    element.scrollTop = 1_400;
    element.dispatchEvent(new Event('scroll'));
  });

  const anchorPath = await page.evaluate(() => {
    const cards = Array.from(document.querySelectorAll<HTMLElement>('.wallpaper-card'));
    return cards.find((card) => {
      const bounds = card.getBoundingClientRect();
      return bounds.bottom > 0 && bounds.top < window.innerHeight;
    })?.dataset.wallpaperPath ?? null;
  });
  expect(anchorPath).toBeTruthy();

  await page.setViewportSize({ width: 1_440, height: 900 });
  await expect.poll(() => page.evaluate((path) => {
    const card = Array.from(document.querySelectorAll<HTMLElement>('.wallpaper-card'))
      .find((candidate) => candidate.dataset.wallpaperPath === path);
    if (!card) return false;
    const bounds = card.getBoundingClientRect();
    return bounds.bottom > 0 && bounds.top < window.innerHeight;
  }, anchorPath!)).toBe(true);
});

test('first run requires explicit folder or Wallpaper Engine confirmation', async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => window.__mockControl?.setFirstRunSourceSuggestions([
    { kind: 'directory', label: 'Downloads', path: '/mock/Downloads' },
    { kind: 'wallpaperEngine', roots: ['/mock/Steam/workshop/content/431960'] },
  ]));
  await openSources(page);
  await removeSource(page, 1);
  await removeSource(page, 2);
  await removeSource(page, 3);
  await page.getByRole('button', { name: 'Close wallpaper sources' }).click();

  await expect(page.getByRole('heading', { name: 'Choose where your wallpapers live' })).toBeVisible();
  await expect(page.getByText('Nothing is scanned until you choose it.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Add Folder' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Suggested sources' })).toBeVisible();
  await expect(page.getByText('Nothing is scanned until you confirm a suggestion.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Add Downloads as a wallpaper source' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Confirm Wallpaper Engine scan' })).toBeVisible();
  await expect(page.locator('.scan-activity')).toHaveCount(0);

  await page.getByRole('button', { name: 'Add Folder' }).click();
  const sourceDialog = page.getByRole('dialog', { name: 'Wallpaper sources' });
  await expect(sourceDialog).toBeVisible();
  await expect(page.getByText('No wallpaper sources yet')).toBeVisible();
  await sourceDialog.getByRole('button', { name: 'Close wallpaper sources' }).click();
  await page.getByRole('button', { name: 'Add Downloads as a wallpaper source' }).click();
  await openSources(page);
  await expect(sourceRow(page, 4)).toContainText('/mock/Downloads');
});
