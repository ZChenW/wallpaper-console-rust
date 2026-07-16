import { expect, test } from '@playwright/test';

test('first Library request reaches cards or an interactive fallback within one second', async ({ page }) => {
  const started = Date.now();
  await page.goto('/');
  await expect(
    page.locator('.wallpaper-card').first().or(page.getByRole('button', { name: 'Retry' })).or(
      page.getByRole('heading', { name: 'Choose where your wallpapers live' }),
    ),
  ).toBeVisible({ timeout: 1_000 });
  const elapsed = Date.now() - started;
  expect(elapsed).toBeLessThanOrEqual(1_000);

  const requestDuration = await page.evaluate(() =>
    performance.getEntriesByName('wc-library-first-request').at(-1)?.duration ?? null,
  );
  expect(requestDuration).not.toBeNull();
  expect(requestDuration!).toBeLessThanOrEqual(1_000);
});
