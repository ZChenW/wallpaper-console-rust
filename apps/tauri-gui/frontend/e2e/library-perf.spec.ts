import { expect, test } from '@playwright/test';

import { TINY_WEBM } from './mediaFixtures.ts';

const FLOW_CENTER_FEEDBACK_BUDGET_MS = 250;
const FLOW_RAIL_ACTIVATION_BUDGET_MS = 150;

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

test('Flow rail activation centers and settles without smooth-scroll latency', async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'The rail timing gate is desktop-only.');
  await page.goto('/');
  await expect(page.locator('.wallpaper-grid')).toBeVisible();
  await page.getByRole('button', { name: 'Flow' }).click();
  await expect(page.locator('.flow-preview-item[data-centered="true"][data-settled="true"]'))
    .toBeVisible();

  const target = page.locator('.flow-index-rail__entry').last();
  const targetId = await target.getAttribute('data-wallpaper-id');
  if (!targetId) throw new Error('Flow rail target has no wallpaper ID');
  const elapsedMs = await target.evaluate(async (button, wallpaperId) => {
    const startedAt = performance.now();
    button.click();
    return new Promise<number>((resolve, reject) => {
      const sample = () => {
        const item = document.querySelector<HTMLElement>(
          `.flow-preview-item[data-wallpaper-id="${wallpaperId}"]`,
        );
        if (item?.dataset.centered === 'true' && item.dataset.settled === 'true') {
          resolve(performance.now() - startedAt);
          return;
        }
        if (performance.now() - startedAt > 1_000) {
          reject(new Error('Flow rail activation did not settle within one second'));
          return;
        }
        requestAnimationFrame(sample);
      };
      requestAnimationFrame(sample);
    });
  }, targetId);

  expect(elapsedMs).toBeLessThanOrEqual(FLOW_RAIL_ACTIVATION_BUDGET_MS);
});

test('Flow rapid rail browsing activates enhanced media only for the final selection', async ({
  page,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'The media churn gate is desktop-only.');
  await page.route('**/mock/path/wallpaper-000.mp4', (route) => route.fulfill({
    body: TINY_WEBM,
    contentType: 'video/webm',
  }));
  await page.goto('/');
  await expect(page.locator('.wallpaper-grid')).toBeVisible();
  await page.getByRole('button', { name: 'Flow' }).click();
  await expect(page.locator('.flow-preview-item[data-centered="true"][data-settled="true"]'))
    .toBeVisible();

  await page.evaluate(() => {
    const stream = document.querySelector('.flow-preview-stream');
    if (!stream) throw new Error('Flow stream unavailable');
    const audit = { activations: 0 };
    const observer = new MutationObserver((records) => {
      const activatedElements = new Set<Element>();
      for (const record of records) {
        if (record.type === 'attributes') {
          const target = record.target;
          if (!(target instanceof Element)) continue;
          if (record.attributeName === 'data-enhanced-preview') {
            if (record.oldValue === null) {
              activatedElements.add(target);
            }
          }
          continue;
        }
        for (const node of record.addedNodes) {
          if (!(node instanceof Element)) continue;
          if (node.matches('[data-enhanced-preview]')) activatedElements.add(node);
          for (const enhanced of node.querySelectorAll('[data-enhanced-preview]')) {
            activatedElements.add(enhanced);
          }
        }
      }
      audit.activations += activatedElements.size;
    });
    observer.observe(stream, {
      attributeFilter: ['data-enhanced-preview'],
      attributeOldValue: true,
      attributes: true,
      childList: true,
      subtree: true,
    });
    Object.assign(window, { __flowMediaAudit: { audit, observer } });
  });

  await page.evaluate(() => {
    for (let index = 0; index < 12; index += 1) {
      const entries = Array.from(
        document.querySelectorAll<HTMLButtonElement>('.flow-index-rail__entry'),
      ).filter((entry) => /\.(?:gif|jpe?g|png|mp4)$/i.test(entry.dataset.wallpaperPath ?? ''));
      const target = index % 2 === 0 ? entries.at(-1) : entries.at(0);
      target?.click();
    }
  });
  const finalPath = '/mock/path/wallpaper-000.mp4';
  await page.locator(
    `.flow-index-rail__entry[data-wallpaper-path="${finalPath}"]`,
  ).click();
  const finalPreview = page.locator(
    `.flow-preview-item[data-wallpaper-path="${finalPath}"]`,
  );
  await expect(finalPreview).toHaveAttribute('data-centered', 'true');
  await expect(finalPreview).toHaveAttribute('data-settled', 'true');
  await expect(finalPreview).toHaveAttribute('aria-selected', 'true');
  await expect(finalPreview.locator('[data-enhanced-preview]')).toHaveCount(1);

  const activations = await page.evaluate(() => {
    const holder = window as unknown as {
      __flowMediaAudit?: {
        audit: { activations: number };
        observer: MutationObserver;
      };
    };
    holder.__flowMediaAudit?.observer.disconnect();
    return holder.__flowMediaAudit?.audit.activations ?? 0;
  });
  expect(activations).toBeLessThanOrEqual(2);
});

test('Flow keeps 5000+ queried wallpapers responsive and its complete index virtualized', async ({
  page,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'The 5000+ Flow DOM budget is a desktop gate.');
  test.setTimeout(90_000);

  await page.addInitScript(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
  });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/');
  await expect(page.locator('.wallpaper-grid')).toBeVisible();

  await page.evaluate(() => {
    const control = (window as unknown as {
      __mockControl?: { setBrowserFixtureCopies(copies: number): void };
    }).__mockControl;
    if (!control) throw new Error('mock control is unavailable');
    control.setBrowserFixtureCopies(34);
  });
  await page.getByLabel('Search wallpapers').fill('wallpaper');
  await expect(page.locator('.single-page-count')).toHaveText('120 / 5100');

  await page.getByRole('button', { name: 'Flow' }).click();
  const stream = page.getByRole('listbox', { name: 'Wallpaper Flow' });
  await expect(stream).toBeVisible();
  await expect(page.locator('.flow-preview-item').first()).toBeVisible();

  const loadedCount = async (): Promise<number> => {
    const text = await page.locator('.single-page-count').textContent();
    const count = Number(text?.split('/')[0].trim());
    if (!Number.isFinite(count)) throw new Error(`Could not parse loaded count from "${text}"`);
    return count;
  };
  const assertBoundedFlowDom = async (): Promise<void> => {
    expect(await page.locator('.flow-preview-item').count()).toBeLessThanOrEqual(15);
    expect(await page.locator('[data-enhanced-preview]').count()).toBeLessThanOrEqual(1);
    expect(await page.locator('video[data-enhanced-preview="video"]').count()).toBeLessThanOrEqual(1);
  };

  await assertBoundedFlowDom();
  while (await loadedCount() < 5_000) {
    const before = await loadedCount();
    await stream.press('End');
    await expect.poll(loadedCount, { timeout: 4_000 }).toBeGreaterThan(before);
    await assertBoundedFlowDom();
  }

  expect(await loadedCount()).toBeGreaterThanOrEqual(5_000);
  await expect(page.locator('.flow-preview-item[data-centered="true"][data-settled="true"]'))
    .toBeVisible();

  const scrollResponse = await stream.evaluate(async (element) => {
    const initialActiveId = element.getAttribute('aria-activedescendant');
    if (!initialActiveId) throw new Error('Flow has no centered active descendant');

    const initialScrollTop = element.scrollTop;
    const maxScrollTop = Math.max(0, element.scrollHeight - element.clientHeight);
    const distance = Math.max(300, element.clientHeight * 0.85);
    const targetScrollTop = initialScrollTop + distance <= maxScrollTop
      ? initialScrollTop + distance
      : Math.max(0, initialScrollTop - distance);
    if (Math.abs(targetScrollTop - initialScrollTop) < 1) {
      throw new Error('Flow stream has no room for a responsiveness sample');
    }

    const startedAt = performance.now();
    let maxRenderedItems = document.querySelectorAll('.flow-preview-item').length;
    element.dispatchEvent(new WheelEvent('wheel', {
      bubbles: true,
      deltaY: targetScrollTop - initialScrollTop,
    }));
    element.scrollTop = targetScrollTop;
    element.dispatchEvent(new Event('scroll'));

    return new Promise<{ elapsedMs: number; maxRenderedItems: number }>((resolve, reject) => {
      const sample = () => {
        const elapsedMs = performance.now() - startedAt;
        maxRenderedItems = Math.max(
          maxRenderedItems,
          document.querySelectorAll('.flow-preview-item').length,
        );
        const activeId = element.getAttribute('aria-activedescendant');
        const activeItem = activeId ? document.getElementById(activeId) : null;
        if (
          activeId !== null
          && activeId !== initialActiveId
          && activeItem?.matches('.flow-preview-item[data-centered="true"]')
        ) {
          resolve({ elapsedMs, maxRenderedItems });
          return;
        }
        if (elapsedMs >= 1_000) {
          reject(new Error('Flow center did not respond to scrolling within one second'));
          return;
        }
        window.requestAnimationFrame(sample);
      };
      window.requestAnimationFrame(sample);
    });
  });
  expect(scrollResponse.elapsedMs).toBeLessThanOrEqual(FLOW_CENTER_FEEDBACK_BUDGET_MS);
  expect(scrollResponse.maxRenderedItems).toBeLessThanOrEqual(15);
  await assertBoundedFlowDom();

  await page.locator('.flow-preview-item[data-centered="true"]').click();
  await assertBoundedFlowDom();

  await page.getByRole('button', { name: /^Index/ }).click();
  const indexDialog = page.getByRole('dialog', { name: 'Loaded wallpapers' });
  await expect(indexDialog).toBeVisible();
  expect(await indexDialog.locator('[data-flow-index]').count()).toBeLessThanOrEqual(40);
  await assertBoundedFlowDom();

  const overflow = await page.evaluate(() => {
    const viewport = document.querySelector<HTMLElement>('.library-viewport');
    const streamElement = document.querySelector<HTMLElement>('.flow-preview-stream');
    return {
      document: document.documentElement.scrollWidth - document.documentElement.clientWidth,
      viewport: viewport ? viewport.scrollWidth - viewport.clientWidth : Number.POSITIVE_INFINITY,
      stream: streamElement
        ? streamElement.scrollWidth - streamElement.clientWidth
        : Number.POSITIVE_INFINITY,
    };
  });
  expect(overflow.document).toBeLessThanOrEqual(1);
  expect(overflow.viewport).toBeLessThanOrEqual(1);
  expect(overflow.stream).toBeLessThanOrEqual(1);
});
