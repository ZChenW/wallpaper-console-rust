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
  await openApp(page);
  await chooseSelect(page, 'Wallpaper type filter', 'Images');

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
  const sourcePanelBounds = await sourceDialog.boundingBox();
  expect(sourcePanelBounds).not.toBeNull();
  expect(sourcePanelBounds!.x).toBeGreaterThanOrEqual(0);
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
  await page.locator('.wallpaper-card__primary').nth(1).click();
  const feedback = page.locator('[data-feedback-card="apply"]');
  await expect(feedback).toBeVisible();

  for (const viewport of [
    { width: 320, height: 568 },
    { width: 390, height: 844 },
  ]) {
    await page.setViewportSize(viewport);
    await expect(activity).toBeVisible();
    await expect(feedback).toBeVisible();
    const overlap = await Promise.all([activity.boundingBox(), feedback.boundingBox()]).then(
      ([scanBounds, feedbackBounds]) => {
        if (!scanBounds || !feedbackBounds) throw new Error('Concurrent feedback bounds unavailable');
        const width = Math.max(0, Math.min(
          scanBounds.x + scanBounds.width,
          feedbackBounds.x + feedbackBounds.width,
        ) - Math.max(scanBounds.x, feedbackBounds.x));
        const height = Math.max(0, Math.min(
          scanBounds.y + scanBounds.height,
          feedbackBounds.y + feedbackBounds.height,
        ) - Math.max(scanBounds.y, feedbackBounds.y));
        return width * height;
      },
    );
    expect(overlap).toBe(0);
    expect(await documentHasNoHorizontalOverflow(page)).toBe(true);
  }
  const cancel = activity.getByRole('button', { name: 'Cancel' });
  await expect(cancel).toBeVisible();
  await expect(cancel).toBeEnabled();
  await cancel.click();
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
