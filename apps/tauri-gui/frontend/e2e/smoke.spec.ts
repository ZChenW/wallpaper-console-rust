import { expect, test } from '@playwright/test';

test('library renders wallpaper grid', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Library' })).toBeVisible();
  await expect(page.locator('.wallpaper-card').first()).toBeVisible();
});

test('WE Web is indexed but unsupported for live apply', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('we_web');
  const card = page.locator('.wallpaper-card').first();
  await expect(card.getByText('WE Web · Unsupported')).toBeVisible();
  await expect(card.getByText(/Web wallpaper — unsupported/)).toBeVisible();
  await card.click({ button: 'right' });
  await expect(page.getByText('Apply', { exact: true })).toHaveCount(0);
  await expect(page.getByText('Apply preview GIF')).toHaveCount(0);
  await expect(page.getByText('Open experimental Chromium preview')).toHaveCount(0);
  await expect(page.getByText('Apply with linux-wallpaperengine')).toHaveCount(0);
  await expect(page.getByText('Open folder')).toBeVisible();
  await expect(page.getByText('Copy Workshop ID')).toBeVisible();
});

test('settings defaults to General with status cards in modal', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('dialog', { name: 'Settings' })).toBeVisible();
  await expect(page.locator('.status-card')).toHaveCount(3);
  await expect(page.getByRole('heading', { name: 'Status' })).toBeVisible();
  // Quick Actions should no longer appear on General page
  await expect(page.getByRole('heading', { name: 'Quick Actions' })).toHaveCount(0);
  // Web Wallpaper Renderer should not appear
  await expect(page.getByText('Web Wallpaper Renderer')).toHaveCount(0);
  await expect(page.getByText('Chromium Preview (Experimental)')).toHaveCount(0);
  // Settings should be a modal, not a regular .view page
  await expect(page.locator('.view.settings-view')).toHaveCount(0);
});

test('settings can navigate to all categories', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  for (const cat of ['Wallpaper', 'Wallpaper Engine', 'Library', 'Database', 'Advanced']) {
    await sidebar.getByRole('button', { name: cat, exact: true }).click();
    await expect(sidebar.getByRole('button', { name: cat, exact: true })).toHaveAttribute('aria-current', 'page');
  }
});

test('settings modal closes back to previous view', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'History' }).click();
  await expect(page.getByRole('heading', { name: 'History' })).toBeVisible();
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('dialog', { name: 'Settings' })).toBeVisible();
  await page.getByRole('button', { name: 'Close settings' }).click();
  await expect(page.getByRole('dialog', { name: 'Settings' })).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'History' })).toBeVisible();
});

test('settings modal closes with escape', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('dialog', { name: 'Settings' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('wallpaper page shows backends and no raw config keys', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await sidebar.getByRole('button', { name: 'Wallpaper', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Backends' })).toBeVisible();
  await expect(page.getByText(/Images and GIFs use awww for/)).toBeVisible();
  await expect(page.getByText('Image fit mode')).toBeVisible();
  // mpvpaper arguments is in collapsed Advanced section — expand to verify
  await page.locator('.settings-advanced summary').click();
  await expect(page.getByText('mpvpaper arguments')).toBeVisible();
  // No raw config keys on Wallpaper page
  await expect(page.locator('.raw-row')).toHaveCount(0);
  // No WE Web renderer settings
  await expect(page.getByText('Web Wallpaper Renderer')).toHaveCount(0);
  // Old labels should be gone
  await expect(page.getByText('awww resize')).toHaveCount(0);
  await expect(page.getByText('mpvpaper options')).toHaveCount(0);
});

test('database page shows confirm dialog for rebuild', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await sidebar.getByRole('button', { name: 'Database', exact: true }).click();
  await page.getByRole('button', { name: 'Rebuild Database' }).click();
  await expect(page.getByText('Re-scan all configured source directories')).toBeVisible();
  await page.getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByText('Re-scan all configured source directories')).toHaveCount(0);
});

test('database has verify backup rebuild restore export buttons', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await sidebar.getByRole('button', { name: 'Database', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Verify Database' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Backup Database' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Rebuild Database' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Restore Backup' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Export Legacy Files' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Export diagnostics' })).toBeVisible();
});

test('advanced page shows raw config keys after checkbox', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await sidebar.getByRole('button', { name: 'Advanced', exact: true }).click();
  await expect(page.locator('.raw-row')).toHaveCount(0);
  await page.locator('.settings-toggle input[type="checkbox"]').check();
  await expect(page.locator('.raw-row').first()).toBeVisible();
  await expect(page.locator('.raw-key').first()).toContainText('awww_resize');
  await expect(page.getByText('Debug logs')).toBeVisible();
});

test('settings is responsive on narrow viewport', async ({ page }) => {
  await page.setViewportSize({ width: 500, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('dialog', { name: 'Settings' })).toBeVisible();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await expect(sidebar).toBeVisible();
  await sidebar.getByRole('button', { name: 'Database', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Verify Database' })).toBeVisible();
});

test('settings disables database actions while diagnostics export is running', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await sidebar.getByRole('button', { name: 'Database', exact: true }).click();
  await page.getByRole('button', { name: 'Export diagnostics' }).click();
  await expect(page.getByRole('button', { name: 'Export diagnostics' })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Verify Database' })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Backup Database' })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Rebuild Database' })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Restore Backup' })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Export Legacy Files' })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Export diagnostics' })).toBeEnabled({ timeout: 5000 });
  await expect(page.getByRole('button', { name: 'Verify Database' })).toBeEnabled();
  await expect(page.getByRole('button', { name: 'Backup Database' })).toBeEnabled();
});

test('single click does not show batch selection toolbar', async ({ page }) => {
  await page.goto('/');
  const card = page.locator('.wallpaper-card').first();
  await card.click();
  await expect(page.getByText(/selected$/)).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Add to Favorites' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Clear' })).toHaveCount(0);
});

test('context menu action shows error feedback on failure', async ({ page }) => {
  await page.goto('/');
  await page.waitForSelector('.wallpaper-card');
  // Right-click the WE Web card (path contains 3650880224, which mock rejects)
  const card = page.locator('.wallpaper-card').filter({ hasText: 'Web title' }).first();
  await card.click({ button: 'right' });
  // Click "Add to Favorites" — mock returns success=false
  await page.getByText('Add to Favorites').click();
  // Error toast should appear
  await expect(page.locator('.toast')).toBeVisible({ timeout: 5000 });
});

test('sources page renders flat list without group headers', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Sources' }).click();
  await expect(page.locator('.source-item').first()).toBeVisible();
  await expect(page.getByText('Other Sources')).toHaveCount(0);
  await expect(page.locator('.source-group-header')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Scan Wallpaper Engine' })).toBeVisible();
});

test('settings wallpaper page shows awww image backend', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await sidebar.getByRole('button', { name: 'Wallpaper', exact: true }).click();
  await expect(page.getByText(/awww for smooth static/)).toBeVisible({ timeout: 3000 });
  // Expand advanced section for frame rate
  await page.locator('.settings-advanced summary').click();
  await expect(page.getByText('Transition frame rate')).toBeVisible({ timeout: 3000 });
});

test('wallpaper engine page shows WE Web unsupported notice', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await sidebar.getByRole('button', { name: 'Wallpaper Engine', exact: true }).click();
  await expect(page.getByText(/Wallpaper Engine.*Web.*indexed/)).toBeVisible();
});

test('settings advanced shows open location mode without ask option', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await sidebar.getByRole('button', { name: 'Advanced', exact: true }).click();
  const select = page.locator('.config-row').filter({ hasText: 'Open project folders with' }).locator('select');
  const options = await select.locator('option').allTextContents();
  expect(options).not.toContain('ask');
  expect(options).toContain('file_manager');
  // Ask-on-first-use callout should be visible by default
  await expect(page.getByText(/ask on first use/)).toBeVisible();
  await select.selectOption({ value: 'terminal' });
  await expect(page.getByRole('heading', { name: 'Terminal File Manager' })).toBeVisible({ timeout: 5000 });
  await select.selectOption({ value: 'file_manager' });
  await expect(page.getByRole('heading', { name: 'File Manager' })).toBeVisible({ timeout: 5000 });
});

test('WE Scene context menu shows all actions', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('we_scene');
  const card = page.locator('.wallpaper-card').filter({ hasText: 'WE Scene' }).first();
  await card.click({ button: 'right' });
  await expect(page.getByText('Apply with linux-wallpaperengine')).toHaveCount(0);
  await expect(page.getByText('Apply', { exact: true })).toBeVisible();
  await expect(page.getByText('Apply preview GIF')).toBeVisible();
  await expect(page.getByText('Open folder')).toBeVisible();
  await expect(page.getByText('Copy Workshop ID')).toBeVisible();
});

test('failed WE Scene context menu shows retry not apply', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('we_scene');
  const card = page.locator('.wallpaper-card').filter({ hasText: 'Incompatible Scene' }).first();
  await card.click({ button: 'right' });
  await expect(page.getByText('Retry backend apply')).toBeVisible();
  await expect(page.getByText('Apply', { exact: true })).toHaveCount(0);
  await expect(page.getByText('Apply preview GIF')).toBeVisible();
  await expect(page.getByText('Open folder')).toBeVisible();
  await expect(page.getByText('Copy Workshop ID')).toBeVisible();
});

test('regular wallpaper context menu has apply and open folder', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('image');
  const card = page.locator('.wallpaper-card').first();
  await card.click({ button: 'right' });
  await expect(page.getByText('Apply', { exact: true })).toBeVisible();
  await expect(page.getByText('Open folder')).toBeVisible();
});

test('favorites context menu shows apply and open folder', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Favorites' }).click();
  await expect(page.locator('h2').filter({ hasText: 'Favorites' })).toBeVisible({ timeout: 5000 });
  await page.locator('.wallpaper-card').first().waitFor({ state: 'visible', timeout: 10000 });
  const card = page.locator('.wallpaper-card').first();
  await card.click({ button: 'right' });
  await expect(page.getByText('Apply', { exact: true })).toBeVisible();
  await expect(page.getByText('Open folder')).toBeVisible();
  await expect(page.getByText('Remove from Favorites')).toBeVisible();
});

test('favorites context menu shows WE Scene actions', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Favorites' }).click();
  await expect(page.locator('h2').filter({ hasText: 'Favorites' })).toBeVisible({ timeout: 5000 });
  await page.locator('.wallpaper-card').first().waitFor({ state: 'visible', timeout: 10000 });
  const card = page.locator('.wallpaper-card').filter({ hasText: 'Scene title' }).first();
  await card.click({ button: 'right' });
  await expect(page.getByText('Apply', { exact: true })).toBeVisible();
  await expect(page.getByText('Apply preview GIF')).toBeVisible();
  await expect(page.getByText('Open folder')).toBeVisible();
  await expect(page.getByText('Copy Workshop ID')).toBeVisible();
  await expect(page.getByText('Apply with linux-wallpaperengine')).toHaveCount(0);
});

test('history context menu shows apply and open folder', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'History' }).click();
  await expect(page.locator('h2').filter({ hasText: 'History' })).toBeVisible({ timeout: 5000 });
  await page.locator('.wallpaper-card').first().waitFor({ state: 'visible', timeout: 10000 });
  const card = page.locator('.wallpaper-card').first();
  await card.click({ button: 'right' });
  await expect(page.getByText('Apply', { exact: true })).toBeVisible();
  await expect(page.getByText('Open folder')).toBeVisible();
});

test('Apply preview GIF completes through explicit preview action', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('we_scene');
  const card = page.locator('.wallpaper-card').filter({ hasText: 'WE Scene' }).first();
  await card.click({ button: 'right' });
  await page.getByText('Apply preview GIF').click();
  await expect(page.locator('.toast')).toContainText(/Applied|Preview/);
});

test('WE Web double click shows cannot apply warning', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('we_web');
  const card = page.locator('.wallpaper-card').filter({ hasText: 'WE Web' }).first();
  await card.dblclick();
  await expect(page.locator('.toast')).toContainText('Cannot apply');
});

test('raw config has no horizontal overflow', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await sidebar.getByRole('button', { name: 'Advanced', exact: true }).click();
  await page.locator('.settings-toggle input[type="checkbox"]').check();
  await expect(page.locator('.raw-row').first()).toBeVisible();
  const raw = page.locator('.settings-raw-config');
  const ok = await raw.evaluate(el => el.scrollWidth <= el.clientWidth + 1);
  expect(ok).toBeTruthy();
});

test('small-viewport sidebar buttons keep stable dimensions while switching categories', async ({ page }) => {
  await page.setViewportSize({ width: 500, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();

  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await expect(sidebar).toBeVisible();

  const display = await sidebar.evaluate(el => window.getComputedStyle(el).display);
  expect(display).toBe('flex');

  const labels = ['General', 'Wallpaper', 'Wallpaper Engine', 'Library', 'Database', 'Advanced'];

  const readBoxes = async () => {
    return sidebar.evaluate((nav) => {
      const result: Record<string, { width: number; height: number }> = {};
      for (const btn of nav.querySelectorAll<HTMLElement>('button')) {
        const text = btn.textContent?.trim() ?? '';
        if (text) result[text] = { width: btn.offsetWidth, height: btn.offsetHeight };
      }
      return result;
    });
  };

  const before = await readBoxes();

  for (const activeLabel of labels) {
    await sidebar.getByRole('button', { name: activeLabel, exact: true }).click();
    const current = await readBoxes();
    for (const label of labels) {
      expect(Math.abs(current[label].width - before[label].width), `${label} width changed after ${activeLabel}`).toBeLessThanOrEqual(1);
      expect(Math.abs(current[label].height - before[label].height), `${label} height changed after ${activeLabel}`).toBeLessThanOrEqual(1);
    }
  }
});

test('mobile tab bar vertical baseline stays stable across all categories', async ({ page }) => {
  await page.setViewportSize({ width: 500, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();

  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  const labels = ['General', 'Wallpaper', 'Wallpaper Engine', 'Library', 'Database', 'Advanced'];

  const measurements: Record<string, { navY: number; navH: number; activeY: number; activeH: number; contentY: number }> = {};

  for (const label of labels) {
    await sidebar.getByRole('button', { name: label, exact: true }).click();

    const navBox = await sidebar.boundingBox();
    const activeBox = await sidebar.locator('button.active').boundingBox();
    const contentBox = await page.locator('.settings-content').boundingBox();

    if (!navBox || !activeBox || !contentBox) throw new Error(`missing layout for ${label}`);

    measurements[label] = {
      navY: navBox.y,
      navH: navBox.height,
      activeY: activeBox.y,
      activeH: activeBox.height,
      contentY: contentBox.y,
    };
  }

  const base = measurements['General'];

  for (const label of labels) {
    expect(Math.abs(measurements[label].navH - base.navH), `${label} nav height`).toBeLessThanOrEqual(1);
    expect(Math.abs(measurements[label].activeY - base.activeY), `${label} active y from nav top`).toBeLessThanOrEqual(1);
    const bottomGap = (base.navY + base.navH) - (base.activeY + base.activeH);
    const currentBottomGap = (measurements[label].navY + measurements[label].navH) - (measurements[label].activeY + measurements[label].activeH);
    expect(Math.abs(currentBottomGap - bottomGap), `${label} bottom gap`).toBeLessThanOrEqual(1);
    expect(Math.abs(measurements[label].contentY - base.contentY), `${label} content y`).toBeLessThanOrEqual(2);
  }
});

test('mobile tab bar divider stays fixed across categories', async ({ page }) => {
  await page.setViewportSize({ width: 500, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();

  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  const labels = ['General', 'Wallpaper', 'Wallpaper Engine', 'Library', 'Database', 'Advanced'];

  let baseBottom: number | null = null;

  for (const label of labels) {
    await sidebar.getByRole('button', { name: label, exact: true }).click();
    const navBox = await sidebar.boundingBox();
    if (!navBox) throw new Error(`missing nav for ${label}`);
    const bottom = navBox.y + navBox.height;
    if (baseBottom === null) {
      baseBottom = bottom;
    } else {
      expect(Math.abs(bottom - baseBottom), `${label} divider bottom`).toBeLessThanOrEqual(1);
    }
  }
});

test('desktop settings sidebar buttons keep stable dimensions while switching categories', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();

  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  const labels = ['General', 'Wallpaper', 'Wallpaper Engine', 'Library', 'Database', 'Advanced'];

  const readBoxes = async () => {
    return sidebar.evaluate((nav) => {
      const result: Record<string, { width: number; height: number }> = {};
      for (const btn of nav.querySelectorAll<HTMLElement>('button')) {
        const text = btn.textContent?.trim() ?? '';
        if (text) result[text] = { width: btn.offsetWidth, height: btn.offsetHeight };
      }
      return result;
    });
  };

  const before = await readBoxes();

  for (const activeLabel of labels) {
    await sidebar.getByRole('button', { name: activeLabel, exact: true }).click();
    const current = await readBoxes();
    for (const label of labels) {
      expect(Math.abs(current[label].width - before[label].width), `${label} width changed after ${activeLabel}`).toBeLessThanOrEqual(1);
      expect(Math.abs(current[label].height - before[label].height), `${label} height changed after ${activeLabel}`).toBeLessThanOrEqual(1);
    }
  }
});

test('settings sidebar keeps stable block from General and Database transitions', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();

  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });

  const boxOf = async (label: string) => {
    const box = await sidebar.getByRole('button', { name: label, exact: true }).boundingBox();
    if (!box) throw new Error(`missing ${label}`);
    return { width: box.width, height: box.height };
  };

  const generalBefore = await boxOf('General');
  await sidebar.getByRole('button', { name: 'Wallpaper', exact: true }).click();
  const generalAfter = await boxOf('General');
  expect(Math.abs(generalAfter.width - generalBefore.width)).toBeLessThanOrEqual(1);
  expect(Math.abs(generalAfter.height - generalBefore.height)).toBeLessThanOrEqual(1);

  await sidebar.getByRole('button', { name: 'Database', exact: true }).click();
  const databaseBefore = await boxOf('Database');
  await sidebar.getByRole('button', { name: 'Wallpaper', exact: true }).click();
  const databaseAfter = await boxOf('Database');
  expect(Math.abs(databaseAfter.width - databaseBefore.width)).toBeLessThanOrEqual(1);
  expect(Math.abs(databaseAfter.height - databaseBefore.height)).toBeLessThanOrEqual(1);
});

test('database maintenance and export sections have no outer card', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await sidebar.getByRole('button', { name: 'Database', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Verify Database' })).toBeVisible();
  await expect(page.locator('.setting-section-plain .section-card')).toHaveCount(0);
});

test('settings pages have compact and consistent top spacing', async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 800 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();

  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  const labels = ['General', 'Wallpaper', 'Wallpaper Engine', 'Library', 'Database', 'Advanced'];

  const measurements: Record<string, { contentTop: number; headerTop: number; sectionTop: number }> = {};

  for (const label of labels) {
    await sidebar.getByRole('button', { name: label, exact: true }).click();

    const content = await page.locator('.settings-content').boundingBox();
    const header = await page.locator('.settings-page-header').boundingBox();
    const section = await page.locator('.setting-section').first().boundingBox();

    if (!content || !header || !section) throw new Error(`missing layout for ${label}`);

    measurements[label] = {
      contentTop: content.y,
      headerTop: header.y,
      sectionTop: section.y,
    };
  }

  const base = measurements['Database'];

  for (const label of labels) {
    expect(Math.abs(measurements[label].headerTop - base.headerTop), `${label} header top`).toBeLessThanOrEqual(1);
    expect(Math.abs(measurements[label].sectionTop - base.sectionTop), `${label} section top`).toBeLessThanOrEqual(8);
  }

  for (const label of ['General', 'Wallpaper']) {
    const topGap = measurements[label].headerTop - measurements[label].contentTop;
    expect(topGap, `${label} top gap`).toBeLessThanOrEqual(18);
  }
});

test('Known Settings card content has safe inset from card border', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();

  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await sidebar.getByRole('button', { name: 'Advanced', exact: true }).click();

  const section = page.locator('.setting-section').filter({ hasText: 'Known Settings' });
  const card = section.locator('.section-card');
  const desc = section.locator('.known-settings-description');

  const cardBox = await card.boundingBox();
  const descBox = await desc.boundingBox();

  if (!cardBox || !descBox) throw new Error('missing Known Settings layout');

  expect(descBox.y - cardBox.y).toBeGreaterThanOrEqual(12);
  expect(descBox.x - cardBox.x).toBeGreaterThanOrEqual(12);
});

test('settings theme defaults to light label', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const themeSelect = page.locator('.config-row').filter({ hasText: 'Theme' }).locator('select');
  await expect(themeSelect).toHaveValue('light');
});

test('settings theme switch applies obsidian warm theme', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const themeSelect = page.locator('.config-row').filter({ hasText: 'Theme' }).locator('select');
  await themeSelect.selectOption('obsidian_warm');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'obsidian_warm');
  const bg = await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue('--bg').trim()
  );
  expect(bg).toBe('#241a14');
});
