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
  await expect(page.getByText('Open experimental Chromium preview')).toHaveCount(0);
  await expect(page.getByText('Apply with linux-wallpaperengine')).toHaveCount(0);
});

test('settings defaults to General with status cards only', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.locator('.status-card')).toHaveCount(3);
  await expect(page.getByRole('heading', { name: 'Status' })).toBeVisible();
  // Quick Actions should no longer appear on General page
  await expect(page.getByRole('heading', { name: 'Quick Actions' })).toHaveCount(0);
  // Web Wallpaper Renderer should not appear
  await expect(page.getByText('Web Wallpaper Renderer')).toHaveCount(0);
  await expect(page.getByText('Chromium Preview (Experimental)')).toHaveCount(0);
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

test('wallpaper page shows backends and no raw config keys', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await sidebar.getByRole('button', { name: 'Wallpaper', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Backends' })).toBeVisible();
  await expect(page.getByText(/Images and GIFs are recommended/)).toBeVisible();
  await expect(page.getByText('awww resize')).toBeVisible();
  // mpvpaper options is in collapsed Advanced section — expand to verify
  await page.locator('.settings-advanced summary').click();
  await expect(page.getByText('mpvpaper options')).toBeVisible();
  // No raw config keys on Wallpaper page
  await expect(page.locator('.raw-row')).toHaveCount(0);
  // No WE Web renderer settings
  await expect(page.getByText('Web Wallpaper Renderer')).toHaveCount(0);
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
  await expect(page.getByRole('navigation', { name: 'Settings categories' })).toBeVisible();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
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

test('batch add favorites does not apply to unsupported items', async ({ page }) => {
  await page.goto('/');
  // Select the WE Web wallpaper
  const weWebCard = page.locator('.wallpaper-card').filter({ hasText: 'WE Web' }).first();
  await weWebCard.click({ modifiers: ['Control'] });
  await expect(page.getByText('1 selected')).toBeVisible();
  // Batch add to favorites should handle unsupported paths correctly
  await page.getByRole('button', { name: 'Add to Favorites' }).click();
  // Selection should be cleared after batch operation
  await expect(page.getByText('1 selected')).toHaveCount(0);
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
  await expect(page.getByText(/awww for smooth/)).toBeVisible({ timeout: 3000 });
  // Expand advanced section for FPS
  await page.locator('.settings-advanced summary').click();
  await expect(page.getByText('Transition FPS')).toBeVisible({ timeout: 3000 });
});

test('wallpaper engine page shows WE Web unsupported notice', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  const sidebar = page.getByRole('navigation', { name: 'Settings categories' });
  await sidebar.getByRole('button', { name: 'Wallpaper Engine', exact: true }).click();
  await expect(page.getByText('Wallpaper Engine Web projects appear in Library for metadata and preview only')).toBeVisible();
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

test('WE Scene context menu uses generic Apply label', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('we_scene');
  const card = page.locator('.wallpaper-card').filter({ hasText: 'WE Scene' }).first();
  await card.click({ button: 'right' });
  await expect(page.getByText('Apply with linux-wallpaperengine')).toHaveCount(0);
  await expect(page.getByText('Apply', { exact: true })).toBeVisible();
});

test('regular wallpaper context menu has open folder', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('image');
  const card = page.locator('.wallpaper-card').first();
  await card.click({ button: 'right' });
  await expect(page.getByText('Open folder')).toBeVisible();
  await expect(page.getByText('Apply', { exact: true })).toBeVisible();
});
