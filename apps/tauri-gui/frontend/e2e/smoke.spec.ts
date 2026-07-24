import { expect, test, type Page } from '@playwright/test';

import { TINY_GIF, TINY_WEBM } from './mediaFixtures.ts';

type TargetedApplyRequest = {
  requestId: string;
  path: string;
  target?: string;
};

type MockControl = {
  injectCommandFailure(command: string): void;
  clearCommandFailure(command: string): void;
  setBrowserFixtureCopies(copies: number): void;
  advanceBrowserRevision(): void;
  delayNextBrowserPage(delayMs: number): void;
  setBrowserPageDelay(delayMs: number): void;
  rejectNextBrowserAppend(message?: string): void;
  emptyNextBrowserAppend(): void;
  browserAppendRequestCount(): number;
  browserPageRequestCount(): number;
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
  holdRuntimeWallpaperObservations(): void;
  releaseRuntimeWallpaperObservations(): void;
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

async function waitForFlow(page: Page): Promise<void> {
  await expect(page.locator('.wallpaper-flow')).toBeVisible();
  await expect(page.getByRole('listbox', { name: 'Wallpaper Flow' })).toBeVisible();
  await expect(page.locator('.flow-preview-item').first()).toBeVisible();
  await expect(page.locator('.flow-preview-item[data-centered="true"][data-settled="true"]'))
    .toBeVisible();
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

async function browserPageRequestCount(page: Page): Promise<number> {
  return page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    return control.browserPageRequestCount();
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

async function gridVisualCenterWallpaperId(page: Page): Promise<string | null> {
  return page.locator('.wallpaper-grid').evaluate((element) => {
    const gridBounds = element.getBoundingClientRect();
    const viewportCenter = gridBounds.top + gridBounds.height / 2;
    const cards = [...element.querySelectorAll<HTMLElement>('.wallpaper-card')]
      .map((card) => ({ card, bounds: card.getBoundingClientRect() }));
    const closest = cards.reduce((winner, candidate) => (
      Math.abs(candidate.bounds.top + candidate.bounds.height / 2 - viewportCenter)
        < Math.abs(winner.bounds.top + winner.bounds.height / 2 - viewportCenter)
        ? candidate
        : winner
    ));
    const row = cards
      .filter(({ bounds }) => Math.abs(bounds.top - closest.bounds.top) < 1)
      .sort((left, right) => left.bounds.left - right.bounds.left);
    return row[Math.floor((row.length - 1) / 2)]?.card.dataset.wallpaperId ?? null;
  });
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

async function chooseSelect(page: Page, label: string, option: string): Promise<void> {
  await page.getByRole('combobox', { name: label }).click();
  await page.getByRole('option', { name: option, exact: true }).click();
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

async function documentHasNoHorizontalOverflow(page: Page): Promise<boolean> {
  return page.evaluate(() =>
    document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1,
  );
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

  await expect(page.locator('.single-page-topbar select, .single-page-filters select')).toHaveCount(0);
  await expect(
    page.locator('.single-page-topbar .select-field-trigger, .single-page-filters .select-field-trigger'),
  ).toHaveCount(5);

  const sourceFilter = page.getByLabel('Source filter');
  const favoriteFilter = page.locator('.single-page-favorite-filter');
  const favoriteCheckbox = page.getByLabel('Favorites');
  await expect(favoriteFilter).toContainText('FAVORITES');
  await expect(favoriteFilter).toHaveAttribute('data-active', 'false');
  await expect(favoriteFilter.locator('svg')).toHaveAttribute('fill', 'none');
  await expect(favoriteFilter).toHaveCSS(
    'border-radius',
    await sourceFilter.evaluate((element) => getComputedStyle(element).borderRadius),
  );
  const sourceFilterBox = await sourceFilter.boundingBox();
  const favoriteFilterBox = await favoriteFilter.boundingBox();
  expect(favoriteFilterBox?.height).toBe(sourceFilterBox?.height);
  const inactiveBackground = await favoriteFilter.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  );
  await favoriteCheckbox.check();
  await expect(favoriteFilter).toHaveAttribute('data-active', 'true');
  await expect(favoriteFilter.locator('svg')).toHaveAttribute('fill', 'currentColor');
  expect(await favoriteFilter.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  )).not.toBe(inactiveBackground);

  await expect(page.locator('.sidebar')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'History', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Favorites', exact: true })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Sources', exact: true })).toHaveCount(0);
});

test('Grid is the first-use default and Flow persists while only one adapter stays mounted', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);

  const viewGroup = page.getByRole('group', { name: 'Library view' });
  const grid = viewGroup.getByRole('button', { name: 'Grid' });
  const flow = viewGroup.getByRole('button', { name: 'Flow' });
  await expect(grid).toHaveAttribute('aria-pressed', 'true');
  await expect(flow).toHaveAttribute('aria-pressed', 'false');

  await flow.click();
  await waitForFlow(page);
  await expect(page.locator('.wallpaper-grid')).toHaveCount(0);
  await expect(flow).toHaveAttribute('aria-pressed', 'true');

  await page.reload();
  await expect(page.locator('.single-page-shell')).toBeVisible();
  await waitForFlow(page);
  await expect(page.locator('.wallpaper-grid')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Flow' })).toHaveAttribute('aria-pressed', 'true');
});

test('Flow completes its direct-start anchor when runtime Current arrives after Library', async ({ page }) => {
  await page.addInitScript(() => {
    window.sessionStorage.setItem('wallpaper-console.mock.hold-runtime-observations', 'true');
  });
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);
  await expect.poll(() => page.evaluate(() => (
    window.sessionStorage.getItem('wallpaper-console.mock.config') ?? ''
  ))).toContain('libraryViewMode');
  await page.reload();
  await expect(page.locator('.single-page-shell')).toBeVisible();
  await expect(page.locator('.library-viewport[data-library-view="flow"]')).toBeVisible();
  await expect(page.locator('.flow-preview-item')).toHaveCount(0);
  await expect(page.getByRole('status')).toHaveText('Preparing Flow preview…');
  await page.evaluate(() => window.__mockControl?.releaseRuntimeWallpaperObservations());

  await waitForFlow(page);
  await expect(page.locator('.single-page-statusbar__current'))
    .toHaveText('Current: wallpaper-001.jpg');
  await expect(page.locator('.flow-preview-item[data-centered="true"]'))
    .toHaveAttribute('data-wallpaper-path', '/mock/path/wallpaper-001.jpg');
});

test('Flow keeps the centered local-index name on the preview anchor near a boundary', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const centerDifference = await page.evaluate(() => {
    const railItem = document.querySelector<HTMLElement>('.flow-index-rail__item[data-centered]');
    const stream = document.querySelector<HTMLElement>('.flow-preview-stream');
    if (!railItem || !stream) throw new Error('Flow alignment targets are unavailable');
    const itemBounds = railItem.getBoundingClientRect();
    const streamBounds = stream.getBoundingClientRect();
    return Math.abs(
      itemBounds.top + itemBounds.height / 2
      - (streamBounds.top + streamBounds.height / 2),
    );
  });
  expect(centerDifference).toBeLessThanOrEqual(2);
});

test('Flow waits for a delayed query replacement before resetting to the first result', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const centered = page.locator('.flow-preview-item[data-centered="true"]');
  const oldAnchorPath = await centered.getAttribute('data-wallpaper-path');
  expect(oldAnchorPath).not.toBeNull();
  await page.evaluate(() => window.__mockControl?.delayNextBrowserPage(600));
  await chooseSelect(page, 'Library sort', 'Name Z–A');
  await page.waitForTimeout(150);
  await expect(centered).toHaveAttribute('data-wallpaper-path', oldAnchorPath!);

  await expect(centered).toHaveAttribute('data-index', '0');
  const newFirstPath = await centered.getAttribute('data-wallpaper-path');
  expect(newFirstPath).not.toBe(oldAnchorPath);
});

test('Flow mounted during a delayed Grid query still resets to the new first result', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);

  const oldAnchorId = await gridVisualCenterWallpaperId(page);
  expect(oldAnchorId).not.toBeNull();
  const oldAnchorPath = await page.locator(
    `.wallpaper-card[data-wallpaper-id="${oldAnchorId!}"]`,
  )
    .getAttribute('data-wallpaper-path');
  expect(oldAnchorPath).not.toBeNull();
  await page.evaluate(() => window.__mockControl?.delayNextBrowserPage(3_000));
  await chooseSelect(page, 'Library sort', 'Name Z–A');
  await page.waitForTimeout(150);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);
  const centered = page.locator('.flow-preview-item[data-centered="true"]');
  await expect(centered).toHaveAttribute('data-wallpaper-path', oldAnchorPath!);

  await expect(centered).toHaveAttribute('data-index', '0');
  const newFirstPath = await centered.getAttribute('data-wallpaper-path');
  expect(newFirstPath).not.toBe(oldAnchorPath);
});

test('Flow scrolling only browses while explicit click and Apply own selection and runtime action', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
  const flow = page.locator('.wallpaper-flow');
  const hoverTarget = page.locator('.flow-preview-item[data-centered="true"]');
  const hoverTargetId = await hoverTarget.getAttribute('data-wallpaper-id');
  expect(hoverTargetId).not.toBeNull();
  const matchingEntry = page.locator(
    `.flow-index-rail__entry[data-wallpaper-id="${hoverTargetId!}"]`,
  );
  const matchingMarker = page.locator(
    `.flow-index-rail__item[data-wallpaper-id="${hoverTargetId!}"]`,
  );
  await hoverTarget.hover();
  await expect(hoverTarget).toHaveAttribute('data-hovered', 'true');
  await expect(matchingEntry).toHaveAttribute('data-hovered', 'true');
  await expect(matchingMarker).toHaveAttribute('data-hovered', 'true');
  await expect(flow).toHaveAttribute('data-hovering', '');
  await page.mouse.move(0, 0);
  await expect.poll(() => flow.getAttribute('data-hovering')).toBeNull();
  await expect.poll(() => hoverTarget.getAttribute('data-hovered')).toBeNull();
  await expect.poll(() => matchingEntry.getAttribute('data-hovered')).toBeNull();
  await expect.poll(() => matchingMarker.getAttribute('data-hovered')).toBeNull();

  const initialActive = await stream.getAttribute('aria-activedescendant');
  await stream.evaluate((element) => {
    element.dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: 640 }));
    element.scrollTop = Math.min(element.scrollHeight, element.clientHeight * 1.4);
    element.dispatchEvent(new Event('scroll'));
  });
  await expect.poll(() => stream.getAttribute('aria-activedescendant')).not.toBe(initialActive);
  await expect(page.locator('.single-page-statusbar__selection')).toHaveText(
    'Select a wallpaper to see its details.',
  );
  expect(await lastApplyRequest(page)).toBeNull();

  const centered = page.locator('.flow-preview-item[data-centered="true"]');
  await centered.click();
  await expect(centered).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('.single-page-statusbar__selection')).toContainText('Selected:');
  expect(await lastApplyRequest(page)).toBeNull();

  const appliedPath = await centered.getAttribute('data-wallpaper-path');
  await page.getByRole('button', { name: 'Apply centered wallpaper' }).click();
  await expect.poll(() => lastApplyRequest(page)).toMatchObject({ path: appliedPath });
});

test('Flow desktop index hover transfers one rule and binds its background', async ({
  page,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'Flow index hover geometry is desktop-only.');

  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const flow = page.locator('.wallpaper-flow');
  const centeredPreview = page.locator('.flow-preview-item[data-centered="true"]');
  const centeredId = await centeredPreview.getAttribute('data-wallpaper-id');
  expect(centeredId).not.toBeNull();
  const centeredMarker = page.locator(
    `.flow-index-rail__item[data-wallpaper-id="${centeredId!}"]`,
  );
  const activeRuleColor = await centeredMarker.evaluate(
    (element) => getComputedStyle(element).borderInlineStartColor,
  );
  const hoverEntry = page.locator(
    `.flow-index-rail__entry:not([data-wallpaper-id="${centeredId!}"])`,
  ).first();
  const idleBackgroundColor = await hoverEntry.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  );
  const hoverId = await hoverEntry.getAttribute('data-wallpaper-id');
  expect(hoverId).not.toBeNull();
  const hoveredMarker = page.locator(
    `.flow-index-rail__item[data-wallpaper-id="${hoverId!}"]`,
  );

  await hoverEntry.hover();
  await expect(flow).toHaveAttribute('data-hovering', '');
  await expect(hoverEntry).toHaveAttribute('data-hovered', 'true');
  await expect(hoveredMarker).toHaveAttribute('data-hovered', 'true');
  await expect(page.locator('.flow-index-rail__item[data-hovered]')).toHaveCount(1);
  await expect.poll(() => hoverEntry.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  )).not.toBe(idleBackgroundColor);
  const hoveredRuleColor = await hoveredMarker.evaluate(
    (element) => getComputedStyle(element).borderInlineStartColor,
  );
  expect(hoveredRuleColor).toBe(activeRuleColor);
  const displacedCenteredRuleColor = await centeredMarker.evaluate(
    (element) => getComputedStyle(element).borderInlineStartColor,
  );
  expect(displacedCenteredRuleColor).not.toBe(hoveredRuleColor);

  await page.mouse.move(0, 0);
  await expect.poll(() => flow.getAttribute('data-hovering')).toBeNull();
  await expect(page.locator('.flow-index-rail__item[data-hovered]')).toHaveCount(0);
  await expect.poll(() => centeredMarker.evaluate(
    (element) => getComputedStyle(element).borderInlineStartColor,
  )).toBe(hoveredRuleColor);
  await expect.poll(() => hoverEntry.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  )).toBe(idleBackgroundColor);
});

test('Flow forced colors keeps exactly one outlined index marker', async ({
  page,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'Flow forced-colors index geometry is desktop-only.');
  await page.emulateMedia({ forcedColors: 'active' });

  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const flow = page.locator('.wallpaper-flow');
  const indexItems = page.locator('.flow-index-rail__item');
  const outlinedMarkerIds = () => indexItems.evaluateAll((elements) => elements
    .filter((element) => {
      const style = getComputedStyle(element);
      return style.outlineStyle !== 'none' && Number.parseFloat(style.outlineWidth) > 0;
    })
    .map((element) => element.getAttribute('data-wallpaper-id')));
  const centeredPreview = page.locator('.flow-preview-item[data-centered="true"]');
  const centeredId = await centeredPreview.getAttribute('data-wallpaper-id');
  expect(centeredId).not.toBeNull();
  const centeredMarker = page.locator(
    `.flow-index-rail__item[data-wallpaper-id="${centeredId!}"]`,
  );
  const hoverEntry = page.locator(
    `.flow-index-rail__entry:not([data-wallpaper-id="${centeredId!}"])`,
  ).first();
  await expect(hoverEntry).toBeVisible();
  const hoverId = await hoverEntry.getAttribute('data-wallpaper-id');
  expect(hoverId).not.toBeNull();
  const hoveredMarker = page.locator(
    `.flow-index-rail__item[data-wallpaper-id="${hoverId!}"]`,
  );

  await expect.poll(outlinedMarkerIds).toEqual([centeredId]);
  await hoverEntry.hover();
  await expect(flow).toHaveAttribute('data-hovering', '');
  await expect(hoveredMarker).toHaveAttribute('data-hovered', 'true');
  await expect.poll(outlinedMarkerIds).toEqual([hoverId]);
  const centeredOutline = await centeredMarker.evaluate((element) => {
    const style = getComputedStyle(element);
    return { style: style.outlineStyle, width: style.outlineWidth };
  });
  expect(centeredOutline.style).toBe('none');
  expect(Number.parseFloat(centeredOutline.width)).toBe(0);
  const hoveredBorderColor = await hoveredMarker.evaluate(
    (element) => getComputedStyle(element).borderInlineStartColor,
  );
  const centeredBorderColor = await centeredMarker.evaluate(
    (element) => getComputedStyle(element).borderInlineStartColor,
  );
  expect(hoveredBorderColor).not.toBe(centeredBorderColor);

  await page.mouse.move(0, 0);
  await expect.poll(() => flow.getAttribute('data-hovering')).toBeNull();
  await expect.poll(outlinedMarkerIds).toEqual([centeredId]);
});

test('Flow owns one enhanced preview and releases video decoders when browsing resumes', async ({ page }) => {
  await page.addInitScript(() => {
    const audit = { loadCalls: 0, pauseCalls: 0 };
    (window as unknown as { __mediaLifecycleAudit?: typeof audit }).__mediaLifecycleAudit = audit;
    const originalLoad = HTMLMediaElement.prototype.load;
    const originalPause = HTMLMediaElement.prototype.pause;
    HTMLMediaElement.prototype.load = function load() {
      audit.loadCalls += 1;
      return originalLoad.call(this);
    };
    HTMLMediaElement.prototype.pause = function pause() {
      audit.pauseCalls += 1;
      return originalPause.call(this);
    };
  });
  await page.route('**/mock/path/wallpaper-000.mp4', (route) => route.fulfill({
    body: TINY_WEBM,
    contentType: 'video/webm',
  }));
  await page.route('**/mock/path/wallpaper-003.gif', (route) => route.fulfill({
    body: TINY_GIF,
    contentType: 'image/gif',
  }));

  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
  await stream.focus();
  await stream.press('Home');
  await expect(page.locator('.flow-preview-item[data-index="0"]'))
    .toHaveAttribute('data-settled', 'true');
  const enhanced = page.locator('[data-enhanced-preview]');
  const initialScene = page.locator('.flow-preview-item[data-centered="true"]');
  await initialScene.click();
  await expect(initialScene).toHaveAttribute('aria-selected', 'true');
  await expect(enhanced).toHaveCount(0);

  await chooseSelect(page, 'Wallpaper type filter', 'Videos');
  const videoItem = page.locator('.flow-preview-item[data-centered="true"]');
  await expect(videoItem).toHaveAttribute(
    'data-wallpaper-path',
    '/mock/path/wallpaper-000.mp4',
  );
  await expect(videoItem).toBeAttached();
  const immediateEnhancedCount = await videoItem.evaluate((element) => {
    (element as HTMLElement).click();
    return element.querySelectorAll('[data-enhanced-preview]').length;
  });
  expect(immediateEnhancedCount).toBe(0);
  await expect(videoItem).toHaveAttribute('data-centered', 'true');
  await expect(videoItem).toHaveAttribute('data-settled', 'true');
  await expect(videoItem).toHaveAttribute('aria-selected', 'true');
  await expect(videoItem.locator('video[data-enhanced-preview="video"]')).toHaveCount(1);
  await expect(enhanced).toHaveCount(1);

  const decoderReleasesBeforeBlur = await page.evaluate(() => {
    const audit = (window as unknown as {
      __mediaLifecycleAudit?: { loadCalls: number; pauseCalls: number };
    }).__mediaLifecycleAudit;
    return Math.min(audit?.loadCalls ?? 0, audit?.pauseCalls ?? 0);
  });
  await page.evaluate(() => window.dispatchEvent(new Event('blur')));
  expect(await page.evaluate(() => document.visibilityState)).toBe('visible');
  await expect(videoItem.locator('video[data-enhanced-preview="video"]')).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => {
    const audit = (window as unknown as {
      __mediaLifecycleAudit?: { loadCalls: number; pauseCalls: number };
    }).__mediaLifecycleAudit;
    return Math.min(audit?.loadCalls ?? 0, audit?.pauseCalls ?? 0);
  })).toBeGreaterThan(decoderReleasesBeforeBlur);
  await page.evaluate(() => window.dispatchEvent(new Event('focus')));
  await expect(videoItem.locator('video[data-enhanced-preview="video"]')).toHaveCount(1);

  await stream.focus();
  await stream.press('ArrowDown');
  await expect(page.locator('.flow-preview-item[data-centered="true"]'))
    .toHaveAttribute('data-settled', 'true');
  await expect(page.locator('video[data-enhanced-preview="video"]')).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => {
    const audit = (window as unknown as {
      __mediaLifecycleAudit?: { loadCalls: number; pauseCalls: number };
    }).__mediaLifecycleAudit;
    return Math.min(audit?.loadCalls ?? 0, audit?.pauseCalls ?? 0);
  })).toBeGreaterThanOrEqual(1);

  await chooseSelect(page, 'Wallpaper type filter', 'GIFs');
  const gifItem = page.locator('.flow-preview-item[data-centered="true"]');
  await expect(gifItem).toHaveAttribute(
    'data-wallpaper-path',
    '/mock/path/wallpaper-003.gif',
  );
  await expect(gifItem).toBeAttached();
  await gifItem.click();
  await expect(gifItem).toHaveAttribute('data-centered', 'true');
  await expect(gifItem).toHaveAttribute('data-settled', 'true');
  await expect(gifItem.locator('img[data-enhanced-preview="image"]')).toHaveCount(1);
  await expect(enhanced).toHaveCount(1);

  await stream.focus();
  await stream.press('ArrowDown');
  await expect(enhanced).toHaveCount(0);
});

test('Flow composite keyboard navigation, index activation, and context menu stay scoped', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
  await stream.focus();
  const first = await stream.getAttribute('aria-activedescendant');
  await stream.press('ArrowDown');
  await expect.poll(() => stream.getAttribute('aria-activedescendant')).not.toBe(first);
  await stream.press('Enter');
  const activeId = await stream.getAttribute('aria-activedescendant');
  await expect(page.locator(`#${activeId}`)).toHaveAttribute('aria-selected', 'true');

  const selectedPath = await page.locator(`#${activeId}`).getAttribute('data-wallpaper-path');
  await stream.press('Control+Enter');
  await expect.poll(() => lastApplyRequest(page)).toMatchObject({ path: selectedPath });

  await stream.press('Shift+F10');
  await expect(page.getByRole('menu', { name: 'Wallpaper actions' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(stream).toBeFocused();

  await page.getByRole('navigation', { name: 'Loaded wallpaper index' })
    .getByRole('button', { name: /^Index/ })
    .click();
  const indexDialog = page.getByRole('dialog', { name: 'Loaded wallpapers' });
  await expect(indexDialog).toBeVisible();
  await indexDialog.locator('[data-flow-index="2"]').click();
  await expect(indexDialog).toHaveCount(0);
  await expect(page.locator('.flow-preview-item[data-index="2"]'))
    .toHaveAttribute('aria-selected', 'true');
  expect((await lastApplyRequest(page))?.path).toBe(selectedPath);
});

test('Flow far navigation keeps its active descendant mounted and settles promptly', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
  await stream.focus();
  await page.evaluate(() => {
    const flow = document.querySelector<HTMLElement>('.flow-preview-stream');
    if (!flow) throw new Error('Flow stream is unavailable');
    const violations: string[] = [];
    const inspect = () => {
      const activeId = flow.getAttribute('aria-activedescendant');
      if (activeId && document.getElementById(activeId) === null) {
        violations.push(activeId);
      }
    };
    const observer = new MutationObserver(inspect);
    observer.observe(flow, {
      attributeFilter: ['aria-activedescendant'],
      attributes: true,
      childList: true,
      subtree: true,
    });
    inspect();
    const audit = { violations, observer };
    (window as unknown as { __flowActiveAudit?: typeof audit }).__flowActiveAudit = audit;
  });

  const startedAt = Date.now();
  await stream.press('End');
  await expect(page.locator('.flow-preview-item[data-index="119"]'))
    .toHaveAttribute('data-centered', 'true', { timeout: 1_000 });
  await expect(page.locator('.flow-preview-item[data-index="119"]'))
    .toHaveAttribute('data-settled', 'true', { timeout: 1_000 });
  expect(Date.now() - startedAt).toBeLessThan(800);
  const violations = await page.evaluate(() => {
    const owner = window as unknown as {
      __flowActiveAudit?: { violations: string[]; observer: MutationObserver };
    };
    const audit = owner.__flowActiveAudit;
    if (!audit) return ['audit missing'];
    audit.observer.disconnect();
    delete owner.__flowActiveAudit;
    return audit.violations;
  });
  expect(violations).toEqual([]);
});

test('Flow keeps metadata action focus while native scrolling changes the centered item', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
  const favorite = page.locator('.flow-metadata-rail__actions')
    .getByRole('button', { name: /^Favorite/ });
  const initialPath = await page.locator('.flow-preview-item[data-centered="true"]')
    .getAttribute('data-wallpaper-path');
  expect(initialPath).toBeTruthy();

  await favorite.focus();
  await expect(favorite).toBeFocused();
  await stream.evaluate((element) => {
    element.scrollTop = Math.min(
      element.scrollHeight - element.clientHeight,
      element.scrollTop + element.clientHeight * 1.5,
    );
    element.dispatchEvent(new Event('scroll'));
  });

  await expect.poll(async () => (
    page.locator('.flow-preview-item[data-centered="true"]')
      .getAttribute('data-wallpaper-path')
  )).not.toBe(initialPath);
  await expect(page.locator('.flow-preview-item[data-centered="true"]'))
    .toHaveAttribute('data-settled', 'true');
  await expect(favorite).toBeFocused();
});

test('Flow keeps keyboard focus on Favorite while its update is pending', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const favorite = page.locator('.flow-metadata-rail__actions')
    .getByRole('button', { name: /^Favorite/ });
  const initialPressed = await favorite.getAttribute('aria-pressed');
  await favorite.focus();
  await favorite.press('Enter');

  await expect(favorite).toBeFocused();
  await expect.poll(() => favorite.getAttribute('aria-pressed')).not.toBe(initialPressed);
  await expect(favorite).toBeFocused();
});

test('Flow preserves loaded results across append failure and exposes its finite end and return', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  await page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    control.rejectNextBrowserAppend('mock Flow append failure');
  });

  const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
  await stream.focus();
  await stream.press('End');

  const retry = page.getByRole('button', { name: 'Retry loading more' });
  await expect(retry).toBeVisible();
  await expect(page.getByRole('alert')).toContainText('mock Flow append failure');
  await expect(page.locator('.single-page-count')).toHaveText('120 / 152');
  await expect.poll(() => browserAppendRequestCount(page)).toBe(1);

  await stream.press('End');
  await page.waitForTimeout(350);
  expect(await browserAppendRequestCount(page)).toBe(1);

  await retry.click();
  await expect(page.locator('.single-page-count')).toHaveText('152 / 152');
  await expect(page.getByText('All 152 wallpapers viewed')).toBeVisible();
  await expect.poll(() => browserAppendRequestCount(page)).toBe(2);

  await stream.focus();
  await stream.press('End');
  const last = page.locator('.flow-preview-item[data-index="151"]');
  await expect(last).toHaveAttribute('data-centered', 'true');
  await expect(last).toHaveAttribute('data-settled', 'true');

  const returnToTop = page.getByRole('button', { name: 'Return to first wallpaper' });
  await expect(returnToTop).toBeVisible();
  await returnToTop.click();
  await expect(page.locator('.flow-preview-item[data-index="0"]'))
    .toHaveAttribute('data-centered', 'true');
  await expect(stream).toBeFocused();
});

test('Flow recovers an invalidated paging revision through an atomic page-one replacement', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const initialPageRequests = await browserPageRequestCount(page);
  await page.evaluate(() => {
    window.__mockControl?.setBrowserPageDelay(120);
    window.__mockControl?.advanceBrowserRevision();
  });
  const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
  await stream.focus();
  await stream.press('End');

  await expect.poll(() => browserPageRequestCount(page)).toBeGreaterThanOrEqual(
    initialPageRequests + 2,
  );
  await expect(page.getByRole('button', { name: 'Retry loading more' })).toHaveCount(0);
  await expect(page.locator('.flow-preview-item[data-index="119"]'))
    .toHaveAttribute('data-centered', 'true');
  await expect(page.locator('.flow-preview-item[data-index="119"]'))
    .toHaveAttribute('data-settled', 'true');
  await expect(page.locator('.single-page-count')).toContainText('/ 152');
  await expect(page.locator('.single-page-stale-results')).toHaveCount(0);

  await expect.poll(() => browserPageRequestCount(page), { timeout: 2_000 }).toBeLessThanOrEqual(
    initialPageRequests + 4,
  );
  // Quiet revision recovery can finish a trailing append after the UI has already
  // settled visually; wait until the page-request counter is idle rather than
  // assuming a single 500ms gap is enough.
  await expect.poll(async () => {
    const before = await browserPageRequestCount(page);
    await page.waitForTimeout(500);
    const after = await browserPageRequestCount(page);
    return before === after ? before : -1;
  }, { timeout: 5_000 }).toBeGreaterThan(0);
});

test('Grid and Flow round trips preserve filters, the selected anchor, and incoming focus', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);

  await chooseSelect(page, 'Source filter', 'Pictures');
  await chooseSelect(page, 'Wallpaper type filter', 'Images');
  await chooseSelect(page, 'Library sort', 'Name Z–A');
  await page.getByLabel('Search wallpapers').fill('wallpaper-00');

  const sourceFilter = page.getByLabel('Source filter');
  const typeFilter = page.getByLabel('Wallpaper type filter');
  const sort = page.getByLabel('Library sort');
  const search = page.getByLabel('Search wallpapers');
  await expect.poll(async () => page.locator('.wallpaper-card').evaluateAll((cards) => (
    cards.length > 1 && cards.every((card) => {
      const path = card.getAttribute('data-wallpaper-path') ?? '';
      const title = card.querySelector('.wallpaper-card__primary')?.getAttribute('title') ?? '';
      return /wallpaper-00\d\.jpg$/.test(path) && title.endsWith('· Pictures');
    })
  ))).toBe(true);

  const selectedCard = page.locator('.wallpaper-card').nth(1);
  const selectedPath = await selectedCard.getAttribute('data-wallpaper-path');
  expect(selectedPath).toBeTruthy();
  await selectedCard.locator('.wallpaper-card__primary').click();
  await expect(selectedCard).toHaveClass(/selected/);

  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);
  const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
  const flowAnchor = page.locator(
    `.flow-preview-item[data-wallpaper-path="${selectedPath!}"]`,
  );
  await expect(stream).toBeFocused();
  await expect(flowAnchor).toHaveAttribute('data-centered', 'true');
  await expect(flowAnchor).toHaveAttribute('aria-selected', 'true');
  await expect(sourceFilter).toHaveAttribute('data-value', 'source:1');
  await expect(typeFilter).toHaveAttribute('data-value', 'image');
  await expect(sort).toHaveAttribute('data-value', 'nameDesc');
  await expect(search).toHaveValue('wallpaper-00');

  await page.getByRole('button', { name: 'Grid' }).click();
  await waitForGrid(page);
  const restoredCard = page.locator(
    `.wallpaper-card[data-wallpaper-path="${selectedPath!}"]`,
  );
  await expect(restoredCard).toHaveClass(/selected/);
  await expect(restoredCard.locator('.wallpaper-card__primary')).toBeFocused();
  await expect(sourceFilter).toHaveAttribute('data-value', 'source:1');
  await expect(typeFilter).toHaveAttribute('data-value', 'image');
  await expect(sort).toHaveAttribute('data-value', 'nameDesc');
  await expect(search).toHaveValue('wallpaper-00');
});

test('Grid hands its visual center to Flow when no wallpaper is selected', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await expect(page.locator('.wallpaper-card.selected')).toHaveCount(0);

  const grid = page.locator('.wallpaper-grid');
  await grid.evaluate((element) => {
    element.scrollTop = Math.min(
      element.scrollHeight - element.clientHeight,
      element.clientHeight * 1.4,
    );
    element.dispatchEvent(new Event('scroll'));
  });
  await expect.poll(() => grid.evaluate((element) => element.scrollTop)).toBeGreaterThan(0);

  const centeredGridWallpaperId = await gridVisualCenterWallpaperId(page);
  expect(centeredGridWallpaperId).not.toBeNull();

  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);
  await expect(page.locator('.flow-preview-item[data-centered="true"]'))
    .toHaveAttribute('data-wallpaper-id', centeredGridWallpaperId!);
});

test('Grid publishes its initial visual center before the first switch to Flow', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await expect(page.locator('.wallpaper-card.selected')).toHaveCount(0);

  const centeredGridWallpaperId = await gridVisualCenterWallpaperId(page);
  expect(centeredGridWallpaperId).not.toBeNull();
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);
  await expect(page.locator('.flow-preview-item[data-centered="true"]'))
    .toHaveAttribute('data-wallpaper-id', centeredGridWallpaperId!);
});

test('Flow settles after a mouse drag is released outside the stream', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
  const bounds = await stream.boundingBox();
  expect(bounds).not.toBeNull();
  await page.mouse.move(bounds!.x + bounds!.width / 2, bounds!.y + bounds!.height / 2);
  await page.mouse.down();
  await expect(page.locator('.wallpaper-flow')).toHaveAttribute('data-scrolling', 'true');
  await page.mouse.move(8, 8);
  await page.mouse.up();

  await expect(page.locator('.wallpaper-flow')).not.toHaveAttribute('data-scrolling', 'true');
  await expect(page.locator('.flow-preview-item[data-centered="true"]'))
    .toHaveAttribute('data-settled', 'true');
});

test.describe('Flow short-window touch layout', () => {
  test.use({ hasTouch: true, viewport: { width: 800, height: 400 } });

test('Flow keeps every metadata action fully reachable in short windows', async ({ page }) => {
  const viewports = [
    { width: 1440, height: 400 },
    { width: 1024, height: 400 },
    { width: 800, height: 400 },
    { width: 780, height: 400 },
    { width: 760, height: 400 },
    { width: 600, height: 400 },
    { width: 480, height: 400 },
    { width: 390, height: 400 },
    { width: 390, height: 320 },
  ];
  await page.setViewportSize(viewports[0]);
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  for (const viewport of viewports) {
    await page.setViewportSize(viewport);
    const rail = page.locator('.flow-metadata-rail');
    const railBounds = await rail.boundingBox();
    expect(railBounds).not.toBeNull();
    for (const action of [
      rail.getByRole('button', { name: 'Apply centered wallpaper' }),
      rail.getByRole('button', { name: /^Favorite/ }),
      rail.getByRole('button', { name: 'Details', exact: true }),
    ]) {
      await expect(action).toBeVisible();
      const bounds = await action.boundingBox();
      expect(bounds).not.toBeNull();
      expect(bounds!.height).toBeGreaterThanOrEqual(44);
      expect(bounds!.y).toBeGreaterThanOrEqual(railBounds!.y - 1);
      expect(bounds!.y + bounds!.height)
        .toBeLessThanOrEqual(railBounds!.y + railBounds!.height + 1);
      expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(viewport.height + 1);
    }
    await expect.poll(async () => (
      (await page.getByRole('listbox', { name: 'Wallpaper Flow' }).boundingBox())?.height ?? 0
    )).toBeGreaterThanOrEqual(96);
  }
});
});

test('Flow double click applies while Details and pointer context preserve selection and focus', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
  await expect(page.locator('.single-page-statusbar__selection')).toHaveText(
    'Select a wallpaper to see its details.',
  );
  await stream.focus();
  await stream.press('Shift+F10');
  const informationMenu = page.getByRole('menu', { name: 'Wallpaper actions' });
  await expect(informationMenu).toBeVisible();
  await informationMenu.getByRole('menuitem', { name: 'Information' }).click();
  const informationDetails = page.locator('.wallpaper-details');
  await expect(informationDetails).toBeVisible();
  await expect(page.locator('.single-page-statusbar__selection')).toHaveText(
    'Select a wallpaper to see its details.',
  );
  await informationDetails.getByRole('button', { name: 'Close wallpaper details' }).click();
  await expect(informationDetails).toHaveCount(0);
  await expect(stream).toBeFocused();

  const centered = page.locator('.flow-preview-item[data-centered="true"]');
  const appliedPath = await centered.getAttribute('data-wallpaper-path');
  expect(appliedPath).toBeTruthy();
  await centered.dblclick();
  await expect.poll(() => lastApplyRequest(page)).toMatchObject({ path: appliedPath! });
  await expect(page.locator(
    `.flow-preview-item[data-wallpaper-path="${appliedPath!}"]`,
  )).toHaveAttribute('aria-selected', 'true');

  await stream.focus();
  await stream.press('ArrowDown');
  await expect.poll(async () => (
    await page.locator('.flow-preview-item[data-centered="true"]')
      .getAttribute('data-wallpaper-path')
  )).not.toBe(appliedPath);
  const browsed = page.locator('.flow-preview-item[data-centered="true"]');
  const browsedPath = await browsed.getAttribute('data-wallpaper-path');
  expect(browsedPath).toBeTruthy();
  await expect(browsed).toHaveAttribute('aria-selected', 'false');

  const detailsButton = page.getByRole('button', { name: 'Details', exact: true });
  await detailsButton.click();
  const details = page.locator('.wallpaper-details');
  await expect(details).toBeVisible();
  await expect(page.locator(
    `.flow-preview-item[data-wallpaper-path="${browsedPath!}"]`,
  )).toHaveAttribute('aria-selected', 'false');
  await expect(page.locator(
    `.flow-preview-item[data-wallpaper-path="${appliedPath!}"]`,
  )).toHaveAttribute('aria-selected', 'true');
  await details.getByRole('button', { name: 'Close wallpaper details' }).click();
  await expect(details).toHaveCount(0);
  await expect(detailsButton).toBeFocused();

  await browsed.click({ button: 'right' });
  const contextMenu = page.getByRole('menu', { name: 'Wallpaper actions' });
  await expect(contextMenu).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(contextMenu).toHaveCount(0);
  await expect(stream).toBeFocused();
  await expect(browsed).toHaveAttribute('aria-selected', 'false');
});

test('Flow keeps its structure and readable primary actions across every explicit theme', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const themes = [
    ['Light', 'light'],
    ['Dark', 'dark'],
    ['Glass', 'glass'],
    ['Editorial', 'editorial'],
  ] as const;

  for (const [label, value] of themes) {
    await openSettings(page);
    const settings = page.getByRole('dialog', { name: 'Settings' });
    await chooseSelect(page, 'Theme', label);
    await expect(page.locator('html')).toHaveAttribute('data-theme', value);
    await settings.getByRole('button', { name: 'Close settings' }).click();
    await expect(settings).toHaveCount(0);

    await waitForFlow(page);
    await expect(page.locator('.wallpaper-flow')).toHaveCount(1);
    await expect(page.locator('.wallpaper-grid')).toHaveCount(0);
    await expect(page.getByRole('navigation', { name: 'Loaded wallpaper index' })
      .getByRole('button', { name: /^Index/ })).toBeVisible();

    const actions = page.locator('.flow-metadata-rail__actions');
    const apply = actions.getByRole('button', { name: 'Apply centered wallpaper' });
    const favorite = actions.getByRole('button', { name: /^Favorite/ });
    const details = actions.getByRole('button', { name: 'Details', exact: true });
    await expect(apply).toBeVisible();
    await expect(apply).toBeEnabled();
    await expect(apply).toHaveText('Apply');
    await expect(favorite).toBeVisible();
    await expect(favorite).toBeEnabled();
    await expect(favorite).toContainText('Favorite');
    await expect(details).toBeVisible();
    await expect(details).toBeEnabled();
    await expect(details).toHaveText('Details');
  }
});

test('authorized preview upgrades Flow and Details without a false unavailable state', async ({ page }) => {
  await page.route('**/mock/path/**', (route) => route.fulfill({
    body: TINY_GIF,
    contentType: 'image/gif',
    status: 200,
  }));
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const imageEntry = page.locator('.flow-preview-item[data-wallpaper-path$=".jpg"]').first();
  await imageEntry.click();
  await expect(imageEntry).toHaveAttribute('data-centered', 'true');
  await expect(imageEntry.locator('[data-enhanced-preview="image"]')).toBeVisible();
  await expect(imageEntry.getByText('Preview unavailable', { exact: true })).toHaveCount(0);

  await page.getByRole('button', { name: 'Details', exact: true }).click();
  const details = page.locator('.wallpaper-details');
  const image = details.locator('img');
  await expect(image).toBeVisible();
  await expect.poll(() => image.evaluate((element) => (
    element instanceof HTMLImageElement ? element.naturalWidth : 0
  ))).toBeGreaterThan(0);
  await expect(details.getByText('Preview unavailable', { exact: true })).toHaveCount(0);
});

test('Flow reports unavailable after both the original and thumbnail fail', async ({ page }) => {
  await page.route('**/mock/path/wallpaper-001.jpg', (route) => route.abort());
  await openApp(page);
  await page.evaluate(() => {
    const control = window.__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    control.setThumbnailFailure('/mock/path/wallpaper-001.jpg');
  });
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const imageEntry = page.locator(
    '.flow-preview-item[data-wallpaper-path="/mock/path/wallpaper-001.jpg"]',
  );
  await imageEntry.click();
  await expect(imageEntry).toHaveAttribute('data-centered', 'true');
  await expect(imageEntry.getByText('Preview unavailable', { exact: true })).toBeVisible();
});

test('Editorial is the first painted theme when persisted preferences load', async ({ page }) => {
  let bridgePatched = false;
  await page.route('**/src/api/mockBridge.ts*', async (route) => {
    const response = await route.fetch();
    let body = await response.text();
    const original = body;
    body = body.replace(
      'configGet: async (key) => configStore[key] ?? defaultConfig[key] ?? "",',
      'configGet: async (key) => { await new Promise((resolve) => setTimeout(resolve, 1000)); if (key === "gui_shell_preferences") return JSON.stringify({ theme: "editorial" }); return configStore[key] ?? defaultConfig[key] ?? ""; },',
    );
    bridgePatched = body !== original;
    await route.fulfill({ response, body });
  });
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  expect(bridgePatched).toBe(true);
  await expect(page.locator('html')).not.toHaveAttribute('data-theme', /.+/, { timeout: 500 });
  await expect(page.locator('#root')).toHaveCSS('visibility', 'hidden');
  await waitForGrid(page);
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'editorial');
  await expect(page.locator('#root')).toHaveCSS('visibility', 'visible');
});

test('Flow honors reduced motion and preserves focus and selection boundaries in forced colors', async ({ page }) => {
  await page.emulateMedia({ forcedColors: 'active', reducedMotion: 'reduce' });
  await openApp(page);
  await waitForGrid(page);
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);

  const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
  await stream.focus();
  await stream.press('ArrowDown');
  await expect(page.locator('.flow-preview-item[data-centered="true"]'))
    .toHaveAttribute('data-settled', 'true');
  await expect(stream).toBeFocused();
  await expect(stream).toHaveCSS('outline-style', 'solid');
  await expect(stream).toHaveCSS('outline-width', '2px');
  await expect(stream).toHaveCSS('scroll-behavior', 'auto');

  const centered = page.locator('.flow-preview-item[data-centered="true"]');
  const offCenter = page.locator('.flow-preview-item:not([data-centered])').first();
  await expect(offCenter).toBeAttached();
  expect(await centered.locator('.flow-preview-item__media').evaluate(
    (element) => getComputedStyle(element).transform,
  )).toBe('none');
  expect(await offCenter.locator('.flow-preview-item__media').evaluate(
    (element) => getComputedStyle(element).transform,
  )).toBe('none');

  await centered.click();
  const selectedMedia = centered.locator('.flow-preview-item__media');
  await expect(selectedMedia).toHaveCSS('outline-style', 'solid');
  await expect(selectedMedia).toHaveCSS('outline-width', '2px');

  await chooseSelect(page, 'Wallpaper type filter', 'Videos');
  const videoItem = page.locator('.flow-preview-item[data-centered="true"]');
  await expect(videoItem).toHaveAttribute(
    'data-wallpaper-path',
    '/mock/path/wallpaper-000.mp4',
  );
  await videoItem.click();
  await expect(videoItem).toHaveAttribute('data-centered', 'true');
  await expect(videoItem).toHaveAttribute('data-settled', 'true');
  await expect(videoItem).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('[data-enhanced-preview]')).toHaveCount(0);
});

test.describe('Flow compact touch layout', () => {
  test.use({ hasTouch: true, viewport: { width: 390, height: 844 } });

  test('Index, actions, and return remain touch reachable without page overflow', async ({ page }) => {
    await openApp(page);
    await waitForGrid(page);
    await page.getByRole('button', { name: 'Flow' }).click();
    await waitForFlow(page);

    const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
    const indexNavigation = page.getByRole('navigation', { name: 'Loaded wallpaper index' });
    const indexButton = indexNavigation.getByRole('button', { name: /^Index/ });
    await expect(indexNavigation.locator('.flow-index-rail__list')).toBeHidden();
    await indexButton.tap();
    const indexDialog = page.getByRole('dialog', { name: 'Loaded wallpapers' });
    await expect(indexDialog).toBeVisible();
    await indexDialog.getByRole('button', { name: 'Close wallpaper index' }).tap();
    await expect(indexDialog).toHaveCount(0);
    await expect(stream).toBeFocused();

    const actions = page.locator('.flow-metadata-rail__actions');
    const apply = actions.getByRole('button', { name: 'Apply centered wallpaper' });
    const favorite = actions.getByRole('button', { name: /^Favorite/ });
    const details = actions.getByRole('button', { name: 'Details', exact: true });
    for (const control of [indexButton, apply, favorite, details]) {
      await expect(control).toBeVisible();
      await expect(control).toBeEnabled();
      await expect.poll(async () => (await control.boundingBox())?.height ?? 0)
        .toBeGreaterThanOrEqual(44);
    }

    const appliedPath = await page.locator('.flow-preview-item[data-centered="true"]')
      .getAttribute('data-wallpaper-path');
    await apply.tap();
    await expect.poll(() => lastApplyRequest(page)).toMatchObject({ path: appliedPath });
    const applyFeedback = page.locator('[data-feedback-card="apply"]');
    await expect(applyFeedback).toBeVisible();
    const [actionsBounds, feedbackBounds] = await Promise.all([
      actions.boundingBox(),
      applyFeedback.boundingBox(),
    ]);
    expect(actionsBounds).not.toBeNull();
    expect(feedbackBounds).not.toBeNull();
    const overlapsActions = actionsBounds !== null && feedbackBounds !== null
      && actionsBounds.x < feedbackBounds.x + feedbackBounds.width
      && actionsBounds.x + actionsBounds.width > feedbackBounds.x
      && actionsBounds.y < feedbackBounds.y + feedbackBounds.height
      && actionsBounds.y + actionsBounds.height > feedbackBounds.y;
    expect(overlapsActions, 'Apply feedback must not cover Flow actions').toBe(false);
    const favoriteBefore = await favorite.getAttribute('aria-pressed');
    await favorite.tap();
    await expect.poll(() => favorite.getAttribute('aria-pressed')).not.toBe(favoriteBefore);
    await details.tap();
    const detailsDialog = page.locator('.wallpaper-details');
    await expect(detailsDialog).toBeVisible();
    await detailsDialog.getByRole('button', { name: 'Close wallpaper details' }).tap();
    await expect(details).toBeFocused();

    await stream.evaluate((element) => {
      element.scrollTop = Math.min(
        element.scrollHeight - element.clientHeight,
        element.clientHeight * 1.5,
      );
      element.dispatchEvent(new Event('scroll'));
    });
    const returnToTop = page.getByRole('button', { name: 'Return to first wallpaper' });
    await expect(returnToTop).toBeVisible();
    await expect.poll(async () => (await returnToTop.boundingBox())?.height ?? 0)
      .toBeGreaterThanOrEqual(44);
    await returnToTop.tap();
    await expect(returnToTop).toHaveCount(0);
    await expect(stream).toBeFocused();
    await expect(page.locator('.flow-preview-item[data-index="0"]'))
      .toHaveAttribute('data-centered', 'true');

    await expect.poll(() => documentHasNoHorizontalOverflow(page)).toBe(true);
    expect(await page.locator('.library-viewport').evaluate((element) => (
      element.scrollWidth <= element.clientWidth + 1
    ))).toBe(true);
  });

  test('required Flow breakpoints keep a usable center stream and reachable actions', async ({ page }) => {
    await openApp(page);
    await waitForGrid(page);
    await page.getByRole('button', { name: 'Flow' }).click();
    await waitForFlow(page);

    for (const viewport of [
      { width: 320, height: 568 },
      { width: 390, height: 844 },
      { width: 760, height: 700 },
      { width: 1024, height: 768 },
      { width: 1440, height: 900 },
    ]) {
      await page.setViewportSize(viewport);
      const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
      await expect.poll(async () => (await stream.boundingBox())?.height ?? 0)
        .toBeGreaterThanOrEqual(96);
      for (const action of [
        page.getByRole('button', { name: 'Apply centered wallpaper' }),
        page.getByRole('button', { name: /^Favorite/ }),
        page.getByRole('button', { name: 'Details', exact: true }),
      ]) {
        const bounds = await action.boundingBox();
        expect(bounds).not.toBeNull();
        expect(bounds!.height).toBeGreaterThanOrEqual(44);
        expect(bounds!.x).toBeGreaterThanOrEqual(0);
        expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(viewport.width + 1);
        expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(viewport.height + 1);
      }
      await expect.poll(() => documentHasNoHorizontalOverflow(page)).toBe(true);
      expect(await page.locator('.library-viewport').evaluate((element) => (
        element.scrollWidth <= element.clientWidth + 1
      ))).toBe(true);
      expect(await page.evaluate(() => ({
        stream: getComputedStyle(document.querySelector('.flow-preview-stream')!).overflowY,
        index: getComputedStyle(document.querySelector('.flow-index-rail')!).overflowY,
        metadata: getComputedStyle(document.querySelector('.flow-metadata-rail')!).overflowY,
      }))).toEqual({ stream: 'auto', index: 'hidden', metadata: 'hidden' });
    }
  });

  test('Flow preserves the centered wallpaper when the viewport crosses compact breakpoints', async ({ page }) => {
    await openApp(page);
    await waitForGrid(page);
    await page.getByRole('button', { name: 'Flow' }).click();
    await waitForFlow(page);

    const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
    await stream.evaluate((element) => {
      element.scrollTop = Math.min(
        element.scrollHeight - element.clientHeight,
        Math.max(3_000, element.clientHeight * 4),
      );
      element.dispatchEvent(new Event('scroll'));
    });
    await expect.poll(async () => Number(
      await page.locator('.flow-preview-item[data-centered="true"][data-settled="true"]')
        .getAttribute('data-index'),
    ), { timeout: 2_500 }).toBeGreaterThan(10);

    const centeredBefore = page.locator('.flow-preview-item[data-centered="true"][data-settled="true"]');
    const wallpaperId = await centeredBefore.getAttribute('data-wallpaper-id');
    const indexBefore = await centeredBefore.getAttribute('data-index');
    expect(wallpaperId).not.toBeNull();
    expect(indexBefore).not.toBeNull();

    await page.setViewportSize({ width: 390, height: 844 });
    await expect.poll(async () => (
      await page.locator('.flow-preview-item[data-centered="true"][data-settled="true"]')
        .getAttribute('data-wallpaper-id')
    ), { timeout: 2_500 }).toBe(wallpaperId);

    await page.setViewportSize({ width: 1440, height: 900 });
    await expect.poll(async () => (
      await page.locator('.flow-preview-item[data-centered="true"][data-settled="true"]')
        .getAttribute('data-wallpaper-id')
    ), { timeout: 800 }).toBe(wallpaperId);
    await expect(page.locator('.flow-preview-item[data-centered="true"][data-settled="true"]'))
      .toHaveAttribute('data-index', indexBefore!);
  });

  test('a long touch before scrolling does not snap back to the previous wallpaper', async ({ page }) => {
    await openApp(page);
    await waitForGrid(page);
    await page.getByRole('button', { name: 'Flow' }).click();
    await waitForFlow(page);

    const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
    await stream.dispatchEvent('pointerdown', {
      buttons: 1,
      isPrimary: true,
      pointerId: 1,
      pointerType: 'touch',
    });
    await page.waitForTimeout(320);
    await stream.evaluate((element) => {
      element.scrollTop = Math.min(
        element.scrollHeight - element.clientHeight,
        Math.max(1_000, element.clientHeight * 1.5),
      );
      element.dispatchEvent(new Event('scroll'));
    });
    await stream.dispatchEvent('pointerup', {
      buttons: 0,
      isPrimary: true,
      pointerId: 1,
      pointerType: 'touch',
    });

    await expect.poll(async () => Number(
      await page.locator('.flow-preview-item[data-centered="true"][data-settled="true"]')
        .getAttribute('data-index'),
    ), { timeout: 2_500 }).toBeGreaterThan(0);
    await expect.poll(() => stream.evaluate((element) => element.scrollTop))
      .toBeGreaterThan(300);
  });
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

  await chooseSelect(page, 'Source filter', 'Pictures');
  await chooseSelect(page, 'Wallpaper type filter', 'Images');
  await chooseSelect(page, 'Library sort', 'Name Z–A');

  await expect(page.locator('.wallpaper-card__primary').first()).toHaveAttribute(
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
  await chooseSelect(page, 'Source filter', 'ALL SOURCES');
  await page.getByLabel('Favorites').check();
  await expect(page.locator('.single-page-count')).toHaveText('1 / 1');
  await expect(page.locator('.wallpaper-card')).toHaveAttribute(
    'data-wallpaper-path',
    '/mock/path/wallpaper-001.jpg',
  );
});

test('card heart adds and removes a favorite without applying the wallpaper', async ({ page }) => {
  let bridgePatched = false;
  await page.route('**/src/api/mockBridge.ts*', async (route) => {
    const response = await route.fetch();
    let body = await response.text();
    const needle = 'thumbnailFor: async (path) => {';
    if (body.includes(needle)) {
      body = body.replace(
        needle,
        `${needle}
          const label = path.split('/').at(-1) ?? path;
          const hue = Array.from(path).reduce((sum, char) => sum + char.charCodeAt(0), 0) % 360;
          const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180">'
            + '<rect width="320" height="180" fill="hsl(' + hue + ' 55% 32%)"/>'
            + '<text x="16" y="96" fill="white" font-size="20">' + label + '</text></svg>';
          return { path, thumbnail: 'data:image/svg+xml,' + encodeURIComponent(svg), cacheHit: false };`,
      );
      bridgePatched = true;
    }
    await route.fulfill({ response, body });
  });
  await openApp(page);
  expect(bridgePatched).toBe(true);
  await chooseSelect(page, 'Wallpaper type filter', 'Images');
  await page.locator('.wallpaper-grid').evaluate((grid) => {
    grid.scrollTop = Math.min(
      grid.scrollHeight - grid.clientHeight,
      grid.clientHeight * 1.5,
    );
    grid.dispatchEvent(new Event('scroll'));
  });
  await page.waitForTimeout(250);

  const visibleReadyPreviewCount = () => page.locator('.wallpaper-card').evaluateAll((cards) => {
    const grid = document.querySelector<HTMLElement>('.wallpaper-grid');
    const gridBounds = grid?.getBoundingClientRect();
    if (!gridBounds) return 0;
    return cards.filter((card) => {
      const cardBounds = card.getBoundingClientRect();
      if (cardBounds.bottom <= gridBounds.top || cardBounds.top >= gridBounds.bottom) return false;
      return Array.from(card.querySelectorAll('img, video')).some((media) => {
        const style = getComputedStyle(media);
        const loaded = media instanceof HTMLImageElement
          ? media.naturalWidth > 0
          : media instanceof HTMLVideoElement
            && media.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA;
        return loaded
          && style.display !== 'none'
          && style.visibility !== 'hidden'
          && Number(style.opacity) > 0.01;
      });
    }).length;
  });
  const visiblePreviewSources = () => page.locator('.wallpaper-card').evaluateAll((cards) => {
    const grid = document.querySelector<HTMLElement>('.wallpaper-grid');
    const gridBounds = grid?.getBoundingClientRect();
    if (!gridBounds) return [];
    return cards.flatMap((card) => {
      const cardBounds = card.getBoundingClientRect();
      if (cardBounds.bottom <= gridBounds.top || cardBounds.top >= gridBounds.bottom) return [];
      const image = card.querySelector<HTMLImageElement>('img');
      const path = card.getAttribute('data-wallpaper-path');
      return path && image?.naturalWidth ? [{ path, src: image.src }] : [];
    });
  });
  await expect.poll(visibleReadyPreviewCount).toBeGreaterThan(2);
  const sourcesBeforeFavorite = await visiblePreviewSources();

  const favoritePath = await page.locator('.wallpaper-card').evaluateAll((cards) => {
    const gridBounds = document.querySelector<HTMLElement>('.wallpaper-grid')
      ?.getBoundingClientRect();
    if (!gridBounds) return null;
    const card = cards.find((candidate) => {
      const bounds = candidate.getBoundingClientRect();
      return bounds.bottom > gridBounds.top
        && bounds.top < gridBounds.bottom
        && candidate.querySelector('[aria-label="Add favorite"]');
    });
    return card?.getAttribute('data-wallpaper-path') ?? null;
  });
  expect(favoritePath).not.toBeNull();
  const card = page.locator(`[data-wallpaper-path="${favoritePath}"]`);
  const addFavorite = card.getByRole('button', { name: 'Add favorite' });
  await expect(card).toBeVisible();
  await page.mouse.move(1, 1);
  await expect(addFavorite).toHaveCSS('opacity', '0');

  await card.hover();
  await expect(addFavorite).toHaveCSS('opacity', '1');
  await expect(addFavorite).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  const applyBeforeFavorite = await lastApplyRequest(page);
  await addFavorite.click();

  const removeFavorite = card.getByRole('button', { name: 'Remove favorite' });
  await expect(removeFavorite).toBeVisible();
  await expect(removeFavorite).toBeEnabled();
  await expect(removeFavorite).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await page.evaluate(() => {
    window.dispatchEvent(new Event('wallpaper-console:library-revision-changed'));
  });
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => {
    requestAnimationFrame(() => resolve());
  })));
  const previewsAfterFavorite = await visibleReadyPreviewCount();
  const sourcesAfterFavorite = await visiblePreviewSources();
  const sourcesBeforeByPath = new Map(
    sourcesBeforeFavorite.map(({ path, src }) => [path, src]),
  );
  const retainedSources = sourcesAfterFavorite.filter(
    ({ path }) => sourcesBeforeByPath.has(path),
  );
  expect(previewsAfterFavorite).toBeGreaterThan(2);
  expect(retainedSources.length).toBeGreaterThan(2);
  for (const { path, src } of retainedSources) {
    expect(src).toBe(sourcesBeforeByPath.get(path));
  }
  expect(await lastApplyRequest(page)).toEqual(applyBeforeFavorite);

  await removeFavorite.click();
  await expect(card.getByRole('button', { name: 'Add favorite' })).toBeAttached();
  expect(await lastApplyRequest(page)).toEqual(applyBeforeFavorite);
});

test('random selection obeys every active filter', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);

  await chooseSelect(page, 'Source filter', 'Pictures');
  await chooseSelect(page, 'Wallpaper type filter', 'Videos');
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
  await chooseSelect(page, 'Wallpaper type filter', 'Images');
  await waitForGrid(page);

  const firstCard = page.locator('.wallpaper-card').nth(0);
  const firstPath = await firstCard.getAttribute('data-wallpaper-path');
  expect(firstPath).toBeTruthy();
  await firstCard.click();
  await expect.poll(() => lastApplyRequest(page)).toMatchObject({ path: firstPath! });

  await openSettings(page);
  await chooseSelect(page, 'Apply gesture', 'Double click');
  await page.getByRole('button', { name: 'Close settings' }).click();
  await chooseSelect(page, 'Display target', 'HDMI-A-1');

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

test('settings select hover is limited to the select trigger', async ({ page }) => {
  await openApp(page);
  await openSettings(page);
  await chooseSelect(page, 'Theme', 'Editorial');

  const themeTrigger = page.locator('.settings-panel .select-field-trigger[aria-label="Theme"]');
  const themeLabel = themeTrigger.locator('xpath=../span[1]');

  await themeLabel.hover();
  expect(await themeTrigger.evaluate((element) => element.matches(':hover'))).toBe(false);

  await themeTrigger.hover();
  expect(await themeTrigger.evaluate((element) => element.matches(':hover'))).toBe(true);
  const editorialTextColor = await page.evaluate(() => {
    const colorProbe = document.createElement('span');
    colorProbe.style.color = 'var(--text)';
    document.body.append(colorProbe);
    const color = getComputedStyle(colorProbe).color;
    colorProbe.remove();
    return color;
  });
  await expect.poll(() => themeTrigger.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  )).toBe(editorialTextColor);
});

test('settings select is not activated by clicking its row text', async ({ page }) => {
  await openApp(page);
  await openSettings(page);

  const themeTrigger = page.locator('.settings-panel .select-field-trigger[aria-label="Theme"]');
  const themeLabel = themeTrigger.locator('xpath=../span[1]');
  await themeTrigger.evaluate((element) => {
    element.setAttribute('data-test-click-count', '0');
    element.addEventListener('click', () => {
      const count = Number(element.getAttribute('data-test-click-count') ?? 0);
      element.setAttribute('data-test-click-count', String(count + 1));
    });
  });

  await themeLabel.click();

  await expect(themeTrigger).toHaveAttribute('data-test-click-count', '0');
  await expect(themeTrigger).toHaveAttribute('aria-expanded', 'false');
  await expect(page.getByRole('option')).toHaveCount(0);
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
  await expect(dialog.locator('select')).toHaveCount(0);
  await expect(dialog.locator('.select-field-trigger')).toHaveCount(6);
  await expect(dialog.getByLabel('Theme')).toHaveAttribute('data-value', 'system');
  await expect(dialog.getByLabel('Apply gesture')).toHaveAttribute('data-value', 'single');

  await chooseSelect(page, 'Theme', 'Light');
  await dialog.getByLabel('Theme').click();
  const selectContent = page.locator('.select-field-content');
  const lightMenuBackground = await selectContent.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  );
  expect(lightMenuBackground).not.toBe('rgba(0, 0, 0, 0)');
  expect(lightMenuBackground).not.toBe('rgb(0, 0, 0)');
  await page.getByRole('option', { name: 'Dark', exact: true }).click();
  await dialog.getByLabel('Theme').click();
  const darkMenuBackground = await selectContent.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  );
  expect(darkMenuBackground).not.toBe('rgba(0, 0, 0, 0)');
  expect(darkMenuBackground).not.toBe('rgb(0, 0, 0)');
  expect(darkMenuBackground).not.toBe(lightMenuBackground);
  await page.keyboard.press('Escape');
  await expect(dialog.getByRole('group', { name: 'Image' }).getByRole('button', { name: 'awww' }))
    .toHaveAttribute('aria-pressed', 'true');
  await expect(dialog.getByRole('group', { name: 'GIF' }).getByRole('button', { name: 'awww' }))
    .toHaveAttribute('aria-pressed', 'true');
  await expect(dialog.getByRole('group', { name: 'Video' }).getByRole('button', { name: 'mpvpaper' }))
    .toHaveAttribute('aria-pressed', 'true');
  await expect(dialog.getByText('Default display', { exact: true })).toHaveCount(0);
  await expect(dialog.getByLabel('Renderer installation status')).toHaveCount(0);
  await expect(dialog.getByLabel('Wallpaper Engine scaling')).toBeDisabled();
  const sourceManagement = dialog.getByRole('button', { name: 'Manage wallpaper sources' });
  await expect(sourceManagement).toBeVisible();
  await expect(sourceManagement).toHaveClass(/settings-navigation-card/);
  await expect(sourceManagement).toContainText('Wallpaper sources');
  const sourceManagementBox = await sourceManagement.boundingBox();
  const sourcesSectionBox = await dialog.locator('[data-settings-group="sources"]').boundingBox();
  expect(sourceManagementBox?.width).toBe(sourcesSectionBox?.width);
  const sourceManagementBackground = await sourceManagement.evaluate(
    (element) => getComputedStyle(element).backgroundColor,
  );
  expect(sourceManagementBackground).not.toBe('rgba(0, 0, 0, 0)');
  await sourceManagement.focus();
  expect(await sourceManagement.evaluate(
    (element) => getComputedStyle(element).boxShadow,
  )).not.toBe('none');
  await expect(dialog.getByText(/\d+ sources? ·/)).toHaveCount(0);

  for (const forbidden of [
    'Database',
    'Cache TTL',
    'Raw config',
    'Runtime stages',
    'Repair Database',
  ]) {
    await expect(dialog.getByText(forbidden, { exact: true })).toHaveCount(0);
  }

  await dialog.getByRole('button', { name: 'Manage wallpaper sources' }).click();
  const sourcesDialog = page.getByRole('dialog', { name: 'Wallpaper sources' });
  const settingsOverlay = page.locator('[data-settings-overlay]');
  const settingsDrawer = settingsOverlay.locator('[aria-label="Settings"]');
  await expect(settingsOverlay).toBeAttached();
  await expect(settingsOverlay).toHaveAttribute('data-obscured', 'true');
  await expect(sourcesDialog).toBeVisible();
  const sourcePanel = page.locator('.source-panel');
  await expect(sourcePanel).toHaveAttribute('data-presentation-phase', 'open');
  expect((await sourcesDialog.boundingBox())?.width)
    .toBeCloseTo((await settingsDrawer.boundingBox())?.width ?? 0, 2);
  await sourcesDialog.getByRole('button', { name: 'Back to settings' }).click();
  await expect(sourcePanel).toHaveAttribute('data-presentation-phase', 'exiting');
  await expect(sourcePanel).toBeHidden({ timeout: 1_000 });
  await expect(dialog).toBeVisible();
  await expect(sourceManagement).toBeFocused();

  await sourceManagement.click();
  await expect(sourcesDialog).toBeVisible();
  await sourcesDialog.getByRole('button', { name: 'Close wallpaper sources' }).click();
  await expect(sourcePanel).toBeHidden({ timeout: 1_000 });
  await expect(settingsOverlay).toBeHidden();
  await expect(page.getByRole('button', { name: 'Open settings' })).toBeFocused();

  await expect(grid).toHaveCSS('overflow-y', 'auto');
});

test('Liquid Glass keeps blur off wallpaper cards and uses white-glass source cards', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await openSettings(page);
  const dialog = page.getByRole('dialog', { name: 'Settings' });

  await chooseSelect(page, 'Theme', 'Glass');
  await expect.poll(() => page.locator('html').getAttribute('data-theme')).toBe('glass');

  const styles = await page.evaluate(() => ({
    topbarBackdrop: getComputedStyle(document.querySelector('.single-page-topbar')!).backdropFilter,
    cardBackdrop: getComputedStyle(document.querySelector('.wallpaper-card')!).backdropFilter,
  }));
  expect(styles.topbarBackdrop).not.toBe('none');
  expect(styles.topbarBackdrop).toMatch(/blur\(/);
  expect(styles.cardBackdrop).toBe('none');

  await dialog.getByRole('button', { name: 'Close settings' }).click();
  await expect(dialog).toBeHidden({ timeout: 1_000 });
  await openSources(page);
  const sourceBackgrounds = await page.locator('[data-source-id]').evaluateAll((rows) =>
    rows.slice(0, 2).map((row) => getComputedStyle(row).backgroundColor),
  );
  expect(sourceBackgrounds).toHaveLength(2);
  for (const background of sourceBackgrounds) {
    const channels = background.match(/[\d.]+/g)?.slice(0, 3).map(Number) ?? [];
    expect(channels, `parse source card background ${background}`).toHaveLength(3);
    expect(channels, `source card background must not be black: ${background}`).not.toEqual([0, 0, 0]);
    // White-glass surface: neutral RGB (equal channels), not the old teal dashboard tint.
    expect(channels[0], `source card background must be neutral white-glass: ${background}`).toBe(channels[1]);
    expect(channels[1], `source card background must be neutral white-glass: ${background}`).toBe(channels[2]);
  }
  await page.getByRole('dialog', { name: 'Wallpaper sources' })
    .getByRole('button', { name: 'Close wallpaper sources' })
    .click();
  await openSettings(page);
  await expect(page.getByRole('dialog', { name: 'Settings' }).getByLabel('Theme'))
    .toHaveAttribute('data-value', 'glass');
});

test('Editorial theme is high-contrast, square, persistent, and independently scoped', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  const initialViewport = page.viewportSize();
  if (!initialViewport) throw new Error('Editorial test requires a viewport');
  await openSettings(page);

  const dialog = page.getByRole('dialog', { name: 'Settings' });
  await chooseSelect(page, 'Theme', 'Editorial');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'editorial');

  const editorialVisuals = await page.evaluate(() => {
    const shell = document.querySelector<HTMLElement>('.single-page-shell');
    const topbar = document.querySelector<HTMLElement>('.single-page-topbar');
    const card = document.querySelector<HTMLElement>('.wallpaper-card');
    const cardPrimary = document.querySelector<HTMLElement>('.wallpaper-card__primary');
    const settings = document.querySelector<HTMLElement>('.settings-panel');
    const themeControl = document.querySelector<HTMLElement>('[aria-label="Theme"]');
    if (!shell || !topbar || !card || !cardPrimary || !settings || !themeControl) {
      throw new Error('Editorial visual contract elements are unavailable');
    }

    const colorChannels = (value: string): [number, number, number] => {
      const rgb = value.match(/^rgba?\(([^)]+)\)$/i)?.[1]
        .split(/[\s,\/]+/)
        .filter(Boolean)
        .slice(0, 3)
        .map(Number);
      if (rgb?.length === 3 && rgb.every(Number.isFinite)) {
        return rgb as [number, number, number];
      }

      const srgb = value.match(/^color\(srgb\s+([^)/]+)/i)?.[1]
        .trim()
        .split(/\s+/)
        .slice(0, 3)
        .map((channel) => Number(channel) * 255);
      if (srgb?.length === 3 && srgb.every(Number.isFinite)) {
        return srgb as [number, number, number];
      }
      throw new Error(`Unsupported computed color: ${value}`);
    };
    const luminance = (value: string) => {
      const linear = colorChannels(value).map((channel) => {
        const normalized = channel / 255;
        return normalized <= 0.04045
          ? normalized / 12.92
          : ((normalized + 0.055) / 1.055) ** 2.4;
      });
      return linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
    };
    const contrast = (element: HTMLElement) => {
      const style = getComputedStyle(element);
      const foreground = luminance(style.color);
      const background = luminance(style.backgroundColor);
      return (Math.max(foreground, background) + 0.05)
        / (Math.min(foreground, background) + 0.05);
    };
    const backdrop = (element: HTMLElement) => {
      const style = getComputedStyle(element);
      return [style.backdropFilter, style.getPropertyValue('-webkit-backdrop-filter')]
        .filter(Boolean);
    };

    return {
      shellContrast: contrast(shell),
      settingsContrast: contrast(settings),
      radii: [card, settings, themeControl].map((element) =>
        Number.parseFloat(getComputedStyle(element).borderTopLeftRadius),
      ),
      backdrops: [topbar, card, settings].flatMap(backdrop),
      primaryReset: {
        background: getComputedStyle(cardPrimary).backgroundColor,
        borderWidth: getComputedStyle(cardPrimary).borderTopWidth,
        fontFamilyMatches: getComputedStyle(cardPrimary).fontFamily === getComputedStyle(card).fontFamily,
        heightRatio: cardPrimary.getBoundingClientRect().height / card.getBoundingClientRect().height,
      },
    };
  });

  expect(editorialVisuals.shellContrast).toBeGreaterThanOrEqual(7);
  expect(editorialVisuals.settingsContrast).toBeGreaterThanOrEqual(7);
  expect(editorialVisuals.radii).toEqual([0, 0, 0]);
  expect(editorialVisuals.backdrops.length).toBeGreaterThanOrEqual(3);
  expect(editorialVisuals.backdrops.every((value) => value === 'none')).toBe(true);
  expect(editorialVisuals.primaryReset).toMatchObject({
    background: 'rgba(0, 0, 0, 0)',
    borderWidth: '0px',
    fontFamilyMatches: true,
  });
  expect(editorialVisuals.primaryReset.heightRatio).toBeGreaterThan(0.95);
  expect(await documentHasNoHorizontalOverflow(page)).toBe(true);

  await dialog.getByRole('button', { name: 'Close settings' }).click();
  await expect(dialog).toBeHidden({ timeout: 1_000 });

  const firstCard = page.locator('.wallpaper-card').first();
  await expect(firstCard).toHaveAttribute('data-wallpaper-index', '01');
  const firstOrdinal = firstCard.locator('.wallpaper-index');
  await expect(firstOrdinal).toHaveText('01');
  await expect(firstOrdinal).toHaveAttribute('aria-hidden', 'true');
  const ordinal = await firstOrdinal.evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      display: style.display,
      opacity: Number(style.opacity),
      visibility: style.visibility,
    };
  });
  expect(ordinal).toMatchObject({
    visibility: 'visible',
  });
  expect(ordinal.display).not.toBe('none');
  expect(ordinal.opacity).toBeGreaterThan(0);

  await firstCard.hover();
  await expect.poll(() => firstCard.evaluate((element) => {
    const style = getComputedStyle(element, '::after');
    return {
      content: style.content.replace(/^['"]|['"]$/g, ''),
      display: style.display,
      opacity: Number(style.opacity),
      visibility: style.visibility,
    };
  })).toMatchObject({
    content: 'Select / apply',
    visibility: 'visible',
  });
  await expect.poll(() => firstCard.evaluate((element) =>
    Number(getComputedStyle(element, '::after').opacity),
  )).toBeGreaterThan(0.5);
  const action = await firstCard.evaluate((element) => {
    const style = getComputedStyle(element, '::after');
    return { display: style.display, opacity: Number(style.opacity) };
  });
  expect(action.display).not.toBe('none');
  expect(action.opacity).toBeGreaterThan(0.5);
  expect(await documentHasNoHorizontalOverflow(page)).toBe(true);

  for (const viewport of [
    { width: 320, height: 568 },
    { width: 760, height: 700 },
    { width: 1024, height: 768 },
    { width: 1440, height: 900 },
  ]) {
    await page.setViewportSize(viewport);
    await expect.poll(() => documentHasNoHorizontalOverflow(page)).toBe(true);
    const brandBounds = await page.locator('.single-page-brand').boundingBox();
    expect(brandBounds).not.toBeNull();
    expect(brandBounds!.x).toBeGreaterThanOrEqual(0);
    expect(brandBounds!.x + brandBounds!.width).toBeLessThanOrEqual(viewport.width + 1);
  }
  await page.setViewportSize(initialViewport);

  const applyTarget = page.locator('.wallpaper-card').nth(1);
  const applyPath = await applyTarget.getAttribute('data-wallpaper-path');
  if (!applyPath) throw new Error('Editorial apply target has no wallpaper path');
  await applyTarget.locator('.wallpaper-card__primary').click();
  await expect.poll(async () => (await lastApplyRequest(page))?.path).toBe(applyPath);

  await expect.poll(() => page.evaluate(() => {
    const rawStore = sessionStorage.getItem('wallpaper-console.mock.config');
    if (!rawStore) return null;
    const store = JSON.parse(rawStore) as Record<string, string>;
    const rawPreferences = store.gui_shell_preferences;
    if (!rawPreferences) return null;
    return (JSON.parse(rawPreferences) as { theme?: string }).theme ?? null;
  })).toBe('editorial');
  await page.reload();
  await waitForGrid(page);
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'editorial');

  await openSettings(page);
  await expect(dialog.getByLabel('Theme')).toHaveAttribute('data-value', 'editorial');

  await dialog.getByRole('button', { name: 'Manage wallpaper sources' }).click();
  const sourceDialog = page.getByRole('dialog', { name: 'Wallpaper sources' });
  await expect(sourceDialog).toBeVisible();
  const sourceClose = sourceDialog.getByRole('button', { name: 'Close wallpaper sources' });
  await expect(sourceClose).toHaveCSS('border-top-style', 'solid');
  await expect(sourceClose).toHaveCSS('border-top-width', '1px');
  await expect(sourceClose).toHaveCSS('border-top-left-radius', '0px');
  const sourceCloseAlignment = await sourceClose.evaluate((button) => {
    const glyph = button.querySelector('span');
    if (!glyph) throw new Error('Wallpaper sources close glyph is unavailable');
    const buttonBounds = button.getBoundingClientRect();
    const glyphBounds = glyph.getBoundingClientRect();
    return {
      x: Math.abs(
        glyphBounds.left + glyphBounds.width / 2
        - (buttonBounds.left + buttonBounds.width / 2),
      ),
      y: Math.abs(
        glyphBounds.top + glyphBounds.height / 2
        - (buttonBounds.top + buttonBounds.height / 2),
      ),
    };
  });
  expect(sourceCloseAlignment.x).toBeLessThanOrEqual(1);
  expect(sourceCloseAlignment.y).toBeLessThanOrEqual(1);
  const sourcePanelBounds = await sourceDialog.boundingBox();
  expect(sourcePanelBounds).not.toBeNull();
  expect(sourcePanelBounds!.x).toBeGreaterThanOrEqual(-1);
  expect(sourcePanelBounds!.x + sourcePanelBounds!.width).toBeLessThanOrEqual(initialViewport.width + 1);
  expect(Number.parseFloat(await sourceDialog.evaluate((element) =>
    getComputedStyle(element).borderTopLeftRadius,
  ))).toBe(0);
  await sourceDialog.getByRole('button', { name: 'Back to settings' }).click();
  await expect(dialog).toBeVisible();

  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.setViewportSize({ width: 1024, height: 768 });
  const reducedMotionPanel = await dialog.evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return { left: rect.left, right: rect.right, viewportWidth: window.innerWidth };
  });
  expect(reducedMotionPanel.left).toBeGreaterThanOrEqual(0);
  expect(reducedMotionPanel.right).toBeLessThanOrEqual(reducedMotionPanel.viewportWidth);
  const reducedMotionCard = await firstCard.evaluate((element) => {
    const image = element.querySelector('img');
    const cardStyle = getComputedStyle(element);
    const imageStyle = image ? getComputedStyle(image) : null;
    return {
      animationName: cardStyle.animationName,
      transitionDuration: cardStyle.transitionDuration,
      imageFilter: imageStyle?.filter ?? 'none',
      imageTransitionDuration: imageStyle?.transitionDuration ?? '0s',
    };
  });
  expect(reducedMotionCard.animationName).toBe('none');
  expect(reducedMotionCard.transitionDuration).toBe('0s');
  expect(reducedMotionCard.imageFilter).toBe('none');
  expect(reducedMotionCard.imageTransitionDuration).toBe('0s');
  expect(await documentHasNoHorizontalOverflow(page)).toBe(true);
  await page.emulateMedia({ reducedMotion: 'no-preference' });

  await chooseSelect(page, 'Theme', 'Dark');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await dialog.getByRole('button', { name: 'Close settings' }).click();
  await expect(dialog).toBeHidden({ timeout: 1_000 });

  const darkCardVisuals = await firstCard.evaluate((element) => ({
    actionContent: getComputedStyle(element, '::after').content,
    radius: Number.parseFloat(getComputedStyle(element).borderTopLeftRadius),
  }));
  expect(darkCardVisuals.radius).toBeGreaterThan(0);
  expect(darkCardVisuals.actionContent).toBe('none');
  expect(await documentHasNoHorizontalOverflow(page)).toBe(true);
});

test('settings and wallpaper actions keep keyboard focus contained and restore it on close', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);
  await openSettings(page);

  const dialog = page.getByRole('dialog', { name: 'Settings' });
  const firstSettingsControl = dialog.getByRole('button', { name: 'Close settings' });
  const lastSettingsControl = dialog.getByRole('button', { name: 'Manage wallpaper sources' });
  await expect(firstSettingsControl).toBeFocused();

  await firstSettingsControl.press('Shift+Tab');
  await expect(lastSettingsControl).toBeFocused();
  await lastSettingsControl.press('Tab');
  await expect(firstSettingsControl).toBeFocused();

  await firstSettingsControl.click();
  await expect(dialog).toBeHidden({ timeout: 1_000 });

  const card = page.locator('.wallpaper-card').first();
  const cardPrimary = card.locator('.wallpaper-card__primary');
  await cardPrimary.focus();
  await cardPrimary.press('Shift+F10');

  const menu = page.getByRole('menu', { name: 'Wallpaper actions' });
  const menuItems = menu.getByRole('menuitem');
  await expect(menu).toBeVisible();
  await expect(menuItems.first()).toBeFocused();

  await page.keyboard.press('ArrowDown');
  await expect(menuItems.nth(1)).toBeFocused();
  await page.keyboard.press('Home');
  await expect(menuItems.first()).toBeFocused();
  await page.keyboard.press('End');
  await expect(menuItems.last()).toBeFocused();
  await page.keyboard.press('Escape');

  await expect(menu).toHaveCount(0);
  await expect(cardPrimary).toBeFocused();

  await cardPrimary.press('Shift+F10');
  await expect(menuItems.first()).toBeFocused();
  await menuItems.first().press('Tab');
  await expect(menu).toHaveCount(0);
  await expect(cardPrimary).not.toBeFocused();
  expect(await page.evaluate(() => document.activeElement !== document.body)).toBe(true);

  await cardPrimary.focus();
  await cardPrimary.press('Shift+F10');
  const information = menu.getByRole('menuitem', { name: 'Information' });
  await information.focus();
  await information.press('Enter');
  const details = page.locator('.wallpaper-details');
  const closeDetails = details.getByRole('button', { name: 'Close wallpaper details' });
  await expect(card).toHaveClass(/selected/);
  await expect(page.locator('.single-page-statusbar__selection')).toContainText('Selected:');
  await expect(closeDetails).toBeFocused();
  await closeDetails.press('Escape');
  await expect(details).toHaveCount(0);
  await expect(cardPrimary).toBeFocused();
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
  await expect(settings.getByText(/4 sources · 1 offline/)).toHaveCount(0);
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
  await chooseSelect(page, 'Wallpaper type filter', 'Unsupported');
  await expect(page.locator('.single-page-count')).toHaveText('2 / 2');

  const webWallpaper = page.locator('.wallpaper-card').filter({ hasText: 'Web title' });
  await webWallpaper.click({ button: 'right' });
  const menu = page.locator('.context-menu');
  await expect(menu.getByRole('menuitem', { name: 'Add to Favorites' })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Open Location' })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Information' })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Limitation Details' })).toBeVisible();
  await expect(menu.getByRole('menuitem', { name: 'Apply', exact: true })).toHaveCount(0);
  await expect(menu.getByText(/Retry|preview|linux-wallpaperengine/i)).toHaveCount(0);

  await menu.getByRole('menuitem', { name: 'Limitation Details' }).click();
  await expect(page.locator('[data-feedback-card="system"]')).toContainText(
    'This wallpaper has a renderer limitation.',
  );
  await expect(page.locator('[data-feedback-progress="system"]')).toBeVisible();
});

test('feedback countdown pauses on hover and resumes to automatic dismissal', async ({ page }) => {
  await openApp(page);
  await chooseSelect(page, 'Wallpaper type filter', 'Images');
  await chooseSelect(page, 'Library sort', 'Name Z–A');
  const card = page.locator('.wallpaper-card').first();
  await expect(card).toBeVisible();
  await card.click({ button: 'right' });
  await page.locator('.context-menu').getByRole('menuitem', { name: 'Add to Favorites' }).click();

  const feedback = page.locator('[data-feedback-card="system"]');
  const progress = feedback.getByRole('progressbar');
  await expect(feedback).toContainText('Added to favorites.');
  await expect(progress).toBeVisible();

  await feedback.hover();
  await page.waitForTimeout(150);
  const pausedAt = Number(await progress.getAttribute('aria-valuenow'));
  await page.waitForTimeout(650);
  expect(Number(await progress.getAttribute('aria-valuenow'))).toBe(pausedAt);

  const dismiss = feedback.getByRole('button', { name: 'Dismiss system notification' });
  await dismiss.focus();
  await page.mouse.move(0, 0);
  await page.waitForTimeout(650);
  expect(Number(await progress.getAttribute('aria-valuenow'))).toBe(pausedAt);

  await page.locator('.single-page-search input').focus();
  await page.waitForTimeout(450);
  expect(Number(await progress.getAttribute('aria-valuenow'))).toBeLessThan(pausedAt);
  await expect(feedback).toHaveCount(0, { timeout: 3_500 });
});

test.describe('Flow compact touch status pressure', () => {
  test.use({ hasTouch: true, viewport: { width: 390, height: 844 } });

test('compact Editorial keeps concurrent scan controls clear of notifications', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openApp(page);
  await waitForGrid(page);
  await openSettings(page);
  await chooseSelect(page, 'Theme', 'Editorial');
  await page.getByRole('dialog', { name: 'Settings' })
    .getByRole('button', { name: 'Close settings' })
    .click();

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
  await expect(activity).toBeVisible({ timeout: 1_000 });
  await page.getByRole('button', { name: 'Flow' }).click();
  await waitForFlow(page);
  await page.setViewportSize({ width: 320, height: 568 });
  const actions = page.locator('.flow-metadata-rail__actions');
  await actions.getByRole('button', { name: 'Apply centered wallpaper' }).click();
  const feedback = page.locator('[data-feedback-card="apply"]');
  await expect(feedback).toBeVisible();
  await actions.getByRole('button', { name: /^Favorite/ }).click();
  await expect(page.locator('[data-feedback-card="scan"]')).toBeVisible();
  await expect(page.locator('[data-feedback-card="system"]')).toBeVisible();
  const feedbackOverlay = page.locator('.feedback-overlay');
  await expect.poll(() => feedbackOverlay.evaluate((element) => (
    element.scrollHeight > element.clientHeight
  ))).toBe(true);
  await expect.poll(() => feedbackOverlay.locator('[data-feedback-card]').evaluateAll((cards) => (
    cards.length === 3
      && cards.every((card) => card.getBoundingClientRect().height >= 40)
  ))).toBe(true);
  const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });

  for (const viewport of [
    { width: 760, height: 400 },
    { width: 600, height: 400 },
    { width: 480, height: 400 },
    { width: 390, height: 400 },
    { width: 320, height: 400 },
    { width: 320, height: 568 },
  ]) {
    await page.setViewportSize(viewport);
    await expect(activity).toBeVisible();
    await expect(feedback).toBeVisible();
    await expect.poll(() => feedbackOverlay.evaluate((element) => (
      element.scrollHeight > element.clientHeight
    ))).toBe(true);
    await expect.poll(() => feedbackOverlay.locator('[data-feedback-card]').evaluateAll((cards) => (
      cards.length === 3
        && cards.every((card) => card.getBoundingClientRect().height >= 40)
    ))).toBe(true);
    const overlaps = await Promise.all([
      activity.boundingBox(),
      feedbackOverlay.boundingBox(),
      actions.boundingBox(),
      page.locator('.flow-metadata-rail').boundingBox(),
      activity.getByText('Scanning wallpapers…', { exact: true }).boundingBox(),
      activity.getByRole('progressbar').boundingBox(),
      activity.getByRole('button', { name: 'Cancel' }).boundingBox(),
    ]).then(([
      scanBounds,
      feedbackBounds,
      actionBounds,
      metadataBounds,
      scanTitleBounds,
      scanProgressBounds,
      scanCancelBounds,
    ]) => {
      if (
        !scanBounds
        || !feedbackBounds
        || !actionBounds
        || !metadataBounds
        || !scanTitleBounds
        || !scanProgressBounds
        || !scanCancelBounds
      ) {
        throw new Error('Concurrent Flow status bounds unavailable');
      }
      const overlapArea = (
        first: typeof scanBounds,
        second: typeof scanBounds,
      ) => {
        const width = Math.max(0, Math.min(
          first.x + first.width,
          second.x + second.width,
        ) - Math.max(first.x, second.x));
        const height = Math.max(0, Math.min(
          first.y + first.height,
          second.y + second.height,
        ) - Math.max(first.y, second.y));
        return width * height;
      };
      return {
        scanFeedback: overlapArea(scanBounds, feedbackBounds),
        scanActions: overlapArea(scanBounds, actionBounds),
        feedbackActions: overlapArea(feedbackBounds, actionBounds),
        scanTitleCancel: overlapArea(scanTitleBounds, scanCancelBounds),
        scanProgressCancel: overlapArea(scanProgressBounds, scanCancelBounds),
        scanContentsWithinActivity: [scanTitleBounds, scanProgressBounds, scanCancelBounds]
          .every((bounds) => (
            bounds.x >= scanBounds.x - 1
            && bounds.y >= scanBounds.y - 1
            && bounds.x + bounds.width <= scanBounds.x + scanBounds.width + 1
            && bounds.y + bounds.height <= scanBounds.y + scanBounds.height + 1
          )),
        actionsWithinMetadata: actionBounds.y >= metadataBounds.y - 1
          && actionBounds.y + actionBounds.height
            <= metadataBounds.y + metadataBounds.height + 1,
        bounds: {
          scan: scanBounds,
          feedback: feedbackBounds,
          actions: actionBounds,
          metadata: metadataBounds,
          scanTitle: scanTitleBounds,
          scanProgress: scanProgressBounds,
          scanCancel: scanCancelBounds,
        },
      };
    });
    const layoutMessage = JSON.stringify(overlaps.bounds);
    expect(overlaps.scanFeedback, `Scan status must not cover Apply feedback: ${layoutMessage}`).toBe(0);
    expect(overlaps.scanActions, `Scan status must not cover Flow actions: ${layoutMessage}`).toBe(0);
    expect(overlaps.feedbackActions, `Apply feedback must not cover Flow actions: ${layoutMessage}`).toBe(0);
    expect(overlaps.scanTitleCancel, `Scan title must not cover Cancel: ${layoutMessage}`).toBe(0);
    expect(overlaps.scanProgressCancel, `Scan progress must not cover Cancel: ${layoutMessage}`).toBe(0);
    expect(overlaps.scanContentsWithinActivity, `Scan contents must stay inside status: ${layoutMessage}`).toBe(true);
    expect(overlaps.actionsWithinMetadata, `Flow actions must not be clipped by metadata: ${layoutMessage}`).toBe(true);
    for (const action of [
      actions.getByRole('button', { name: 'Apply centered wallpaper' }),
      actions.getByRole('button', { name: /^Favorite/ }),
      actions.getByRole('button', { name: 'Details', exact: true }),
    ]) {
      const bounds = await action.boundingBox();
      expect(bounds).not.toBeNull();
      expect(bounds!.height).toBeGreaterThanOrEqual(44);
    }
    await expect.poll(async () => (await stream.boundingBox())?.height ?? 0)
      .toBeGreaterThanOrEqual(96);
    expect(await documentHasNoHorizontalOverflow(page)).toBe(true);
  }
  const systemFeedback = page.locator('[data-feedback-card="system"]');
  await systemFeedback.scrollIntoViewIfNeeded();
  const [overlayBounds, systemBounds] = await Promise.all([
    feedbackOverlay.boundingBox(),
    systemFeedback.boundingBox(),
  ]);
  expect(overlayBounds).not.toBeNull();
  expect(systemBounds).not.toBeNull();
  expect(systemBounds!.y).toBeGreaterThanOrEqual(overlayBounds!.y - 1);
  expect(systemBounds!.y + systemBounds!.height)
    .toBeLessThanOrEqual(overlayBounds!.y + overlayBounds!.height + 1);
  const cancel = activity.getByRole('button', { name: 'Cancel' });
  await expect(cancel).toBeVisible();
  await expect(cancel).toBeEnabled();
  await cancel.click();
});
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
  await chooseSelect(page, 'Library sort', 'Name Z–A');
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

  await chooseSelect(page, 'Wallpaper type filter', 'Images');
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
  await chooseSelect(page, 'Library sort', 'Name Z–A');
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
  await chooseSelect(page, 'Library sort', 'Name Z–A');
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
  await chooseSelect(page, 'Library sort', 'Name Z–A');
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
  await chooseSelect(page, 'Library sort', 'Name A–Z');

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

test('current wallpaper keeps its semantics without a special green card border', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);

  const currentControl = page.locator('.wallpaper-card__primary[aria-current="true"]');
  const current = page.locator('.wallpaper-card:has(.wallpaper-card__primary[aria-current="true"])');
  const neutral = page.locator('.wallpaper-card:not(:has(.wallpaper-card__primary[aria-current="true"]))').first();
  await expect(current).toHaveCount(1);
  await expect(currentControl).toHaveAttribute('aria-current', 'true');
  await expect(neutral).toBeVisible();

  const [currentBorder, neutralBorder] = await Promise.all([
    current.evaluate((element) => getComputedStyle(element).borderColor),
    neutral.evaluate((element) => getComputedStyle(element).borderColor),
  ]);
  expect(currentBorder).toBe(neutralBorder);
});

test('success countdown moves continuously between animation frames', async ({ page }) => {
  await openApp(page);
  await waitForGrid(page);

  await page.locator('[data-wallpaper-path="/mock/path/wallpaper-002.jpg"]').click();
  const progress = page.locator('[data-feedback-progress="apply"]');
  const fill = progress.locator('span');
  await expect(progress).toBeVisible();

  const accentJoin = await progress.evaluate((element) => {
    const card = element.closest<HTMLElement>('[data-feedback-card="apply"]');
    if (!card) throw new Error('apply feedback card is unavailable');
    const cardStyle = getComputedStyle(card);
    const accentStyle = getComputedStyle(card, '::before');
    const connectorStyle = getComputedStyle(card, '::after');
    const fill = element.querySelector<HTMLElement>('.feedback-overlay__progress-fill');
    if (!fill) throw new Error('apply feedback progress fill is unavailable');
    const fillStyle = getComputedStyle(fill);
    const progressStyle = getComputedStyle(element);
    return {
      accentBottom: accentStyle.bottom,
      accentColour: accentStyle.backgroundColor,
      accentLeft: accentStyle.left,
      accentWidth: accentStyle.width,
      cardBorderLeftWidth: cardStyle.borderLeftWidth,
      cardBorderRightWidth: cardStyle.borderRightWidth,
      connectorContent: connectorStyle.content,
      progressColour: fillStyle.backgroundColor,
      progressHeight: progressStyle.height,
      progressMarginLeft: progressStyle.marginLeft,
      progressRadius: fillStyle.borderTopLeftRadius,
    };
  });
  expect(accentJoin).toEqual({
    accentBottom: '0px',
    accentColour: accentJoin.progressColour,
    accentLeft: '0px',
    accentWidth: '4px',
    cardBorderLeftWidth: accentJoin.cardBorderRightWidth,
    cardBorderRightWidth: accentJoin.cardBorderRightWidth,
    connectorContent: 'none',
    progressColour: accentJoin.progressColour,
    progressHeight: accentJoin.progressHeight,
    progressMarginLeft: '4px',
    progressRadius: '3.2px',
  });

  const movement = await fill.evaluate((element) => new Promise<{
    frames: number;
    movingFrames: number;
    ratio: number;
  }>((resolve) => {
    const widths: number[] = [];
    const startedAt = performance.now();
    const sample = (now: number) => {
      widths.push(element.getBoundingClientRect().width);
      if (now - startedAt < 700) {
        requestAnimationFrame(sample);
        return;
      }
      const movingFrames = widths.slice(1).filter((width, index) => (
        Math.abs(width - widths[index]) > 0.01
      )).length;
      const frames = Math.max(0, widths.length - 1);
      resolve({
        frames,
        movingFrames,
        ratio: frames === 0 ? 0 : movingFrames / frames,
      });
    };
    requestAnimationFrame(sample);
  }));

  expect(movement.frames).toBeGreaterThan(20);
  expect(movement.ratio).toBeGreaterThanOrEqual(0.95);
});
