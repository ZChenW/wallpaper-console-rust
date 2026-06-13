import { expect, test } from '@playwright/test';

test('library renders wallpaper grid', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Library' })).toBeVisible();
  await expect(page.locator('.wallpaper-card').first()).toBeVisible();
});

test('WE Web uses native renderer action and keeps Chromium preview separate', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('combobox').first().selectOption('we_web');
  const card = page.locator('.wallpaper-card').first();
  await expect(card.getByText('WE Web', { exact: true })).toBeVisible();
  await expect(card.getByText(/native renderer/)).toBeVisible();
  await card.click({ button: 'right' });
  await expect(page.getByText('Apply Web wallpaper')).toBeVisible();
  await expect(page.getByText('Open experimental Chromium preview')).toBeVisible();
  await expect(page.getByText('Apply with linux-wallpaperengine')).toHaveCount(0);
});

test('settings exposes native Web renderer status', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('heading', { name: 'Web Wallpaper Renderer' })).toBeVisible();
  await expect(page.getByText('Native Web renderer: Ready')).toBeVisible();
  await expect(page.getByText('Chromium Preview (Experimental)')).toBeVisible();
});
