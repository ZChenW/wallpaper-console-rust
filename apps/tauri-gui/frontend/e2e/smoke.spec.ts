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
  await expect(page.getByText('Apply Web wallpaper')).toHaveCount(0);
  await expect(page.getByText('Open experimental Chromium preview')).toHaveCount(0);
  await expect(page.getByText('Apply with linux-wallpaperengine')).toHaveCount(0);
});

test('settings explains WE Web unsupported status', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings' }).click();
  await expect(page.getByRole('heading', { name: 'Wallpaper Engine Web Projects' })).toBeVisible();
  await expect(page.getByText(/unsupported as live wallpapers/)).toBeVisible();
  await expect(page.getByText('Web Wallpaper Renderer')).toHaveCount(0);
  await expect(page.getByText('Chromium Preview (Experimental)')).toHaveCount(0);
});
