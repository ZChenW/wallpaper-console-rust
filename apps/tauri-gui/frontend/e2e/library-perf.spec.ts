import { expect, test } from '@playwright/test';

import { TINY_GIF, TINY_WEBM } from './mediaFixtures.ts';

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

test('Grid selection is Flow first-painted geometric center', async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'The first-paint geometry gate is desktop-only.');
  await page.goto('/');
  await expect(page.locator('.wallpaper-grid')).toBeVisible();

  const selectedCard = page.locator('.wallpaper-card').nth(5);
  const selectedPath = await selectedCard.getAttribute('data-wallpaper-path');
  if (!selectedPath) throw new Error('Grid selection target has no wallpaper path');
  await selectedCard.locator('.wallpaper-card__primary').click();
  await expect(selectedCard).toHaveClass(/selected/);

  const firstPaint = await page.evaluate(async (targetPath) => {
    const flowButton = Array.from(document.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => button.textContent?.trim() === 'Flow');
    if (!flowButton) throw new Error('Flow mode button unavailable');
    const startedAt = performance.now();
    flowButton.click();

    return new Promise<{
      elapsedMs: number;
      geometricPath: string | null;
      centeredPath: string | null;
      centerDelta: number;
    }>((resolve, reject) => {
      const timeout = window.setTimeout(
        () => reject(new Error('Flow did not paint within one second')),
        1_000,
      );
      const observer = new MutationObserver(() => {
        const stream = document.querySelector<HTMLElement>('.flow-preview-stream');
        const items = Array.from(document.querySelectorAll<HTMLElement>('.flow-preview-item'));
        if (!stream || items.length === 0) return;
        observer.disconnect();
        window.clearTimeout(timeout);
        window.requestAnimationFrame(() => {
          const streamBounds = stream.getBoundingClientRect();
          const streamCenter = streamBounds.top + streamBounds.height / 2;
          const ranked = items.map((item) => {
            const bounds = item.getBoundingClientRect();
            return {
              item,
              delta: Math.abs(bounds.top + bounds.height / 2 - streamCenter),
            };
          }).sort((left, right) => left.delta - right.delta);
          resolve({
            elapsedMs: performance.now() - startedAt,
            geometricPath: ranked[0]?.item.dataset.wallpaperPath ?? null,
            centeredPath: document.querySelector<HTMLElement>(
              '.flow-preview-item[data-centered="true"]',
            )?.dataset.wallpaperPath ?? null,
            centerDelta: ranked[0]?.delta ?? Number.POSITIVE_INFINITY,
          });
        });
      });
      observer.observe(document.body, { childList: true, subtree: true });
    });
  }, selectedPath);

  expect(firstPaint.elapsedMs).toBeLessThanOrEqual(150);
  expect(firstPaint.centeredPath).toBe(selectedPath);
  expect(firstPaint.geometricPath).toBe(selectedPath);
  expect(firstPaint.centerDelta).toBeLessThanOrEqual(24);
});

test('Flow rapid native scrolling settles the final large-screen center promptly', async ({
  page,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'The native-scroll timing gate is desktop-only.');
  await page.goto('/');
  await expect(page.locator('.wallpaper-grid')).toBeVisible();
  await page.getByRole('button', { name: 'Flow' }).click();
  const settled = page.locator('.flow-preview-item[data-centered="true"][data-settled="true"]');
  await expect(settled).toBeVisible();

  const audit = await page.locator('.flow-preview-stream').evaluate(async (stream) => {
    const initialActiveId = stream.getAttribute('aria-activedescendant');
    if (!initialActiveId) throw new Error('Flow has no initial active descendant');
    const startedAt = performance.now();
    stream.dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: 720 }));
    stream.scrollTop = Math.min(
      stream.scrollHeight - stream.clientHeight,
      stream.scrollTop + Math.max(720, stream.clientHeight * 0.9),
    );
    stream.dispatchEvent(new Event('scroll'));

    await new Promise<void>((resolve) => requestAnimationFrame(() => {
      requestAnimationFrame(() => resolve());
    }));
    const streamBounds = stream.getBoundingClientRect();
    const streamCenter = streamBounds.top + streamBounds.height / 2;
    const geometricItem = Array.from(
      stream.querySelectorAll<HTMLElement>('.flow-preview-item'),
    ).sort((left, right) => {
      const leftBounds = left.getBoundingClientRect();
      const rightBounds = right.getBoundingClientRect();
      return Math.abs(leftBounds.top + leftBounds.height / 2 - streamCenter)
        - Math.abs(rightBounds.top + rightBounds.height / 2 - streamCenter);
    })[0];
    const geometricOrdinal = geometricItem
      ? Number(geometricItem.dataset.index) + 1
      : null;
    const railOrdinalText = document.querySelector<HTMLElement>(
      '.flow-index-rail__item[data-centered] .flow-index-rail__ordinal',
    )?.textContent;
    const moving = {
      geometricOrdinal,
      railOrdinal: railOrdinalText === undefined ? null : Number(railOrdinalText),
      visibleOrdinals: Array.from(
        document.querySelectorAll<HTMLElement>('.flow-index-rail__ordinal'),
      ).map((ordinal) => Number(ordinal.textContent)),
    };

    const elapsedMs = await new Promise<number>((resolve, reject) => {
      const sample = () => {
        const activeId = stream.getAttribute('aria-activedescendant');
        const activeItem = activeId ? document.getElementById(activeId) : null;
        if (
          activeId !== null
          && activeId !== initialActiveId
          && activeItem?.matches('[data-centered="true"][data-settled="true"]')
        ) {
          resolve(performance.now() - startedAt);
          return;
        }
        if (performance.now() - startedAt > 1_000) {
          reject(new Error('Flow did not settle rapid native scrolling within one second'));
          return;
        }
        window.requestAnimationFrame(sample);
      };
      window.requestAnimationFrame(sample);
    });
    return { elapsedMs, moving };
  });

  expect(audit.moving.geometricOrdinal).not.toBeNull();
  expect(audit.moving.railOrdinal).toBe(audit.moving.geometricOrdinal);
  expect(audit.moving.visibleOrdinals).toContain(audit.moving.geometricOrdinal);
  expect(audit.elapsedMs).toBeLessThanOrEqual(350);
});

test('Flow short touchpad flick delays selection and eases the final centering correction', async ({
  page,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'The touchpad motion gate is desktop-only.');
  await page.goto('/');
  await expect(page.locator('.wallpaper-grid')).toBeVisible();
  await page.getByRole('button', { name: 'Flow' }).click();
  await expect(page.locator('.flow-preview-item[data-centered="true"][data-settled="true"]'))
    .toBeVisible();

  const motion = await page.locator('.flow-preview-stream').evaluate(async (stream) => {
    const initialActiveId = stream.getAttribute('aria-activedescendant');
    if (!initialActiveId) throw new Error('Flow has no initial active descendant');
    const initialItem = document.getElementById(initialActiveId);
    if (!initialItem) throw new Error('Flow active descendant is unavailable');
    const items = Array.from(stream.querySelectorAll<HTMLElement>('.flow-preview-item'))
      .sort((left, right) => left.getBoundingClientRect().top - right.getBoundingClientRect().top);
    const initialIndex = items.indexOf(initialItem);
    const targetItem = items[initialIndex + 1] ?? items[initialIndex - 1];
    if (!targetItem) throw new Error('Flow has no adjacent item for a touchpad sample');

    const streamBounds = stream.getBoundingClientRect();
    const targetBounds = targetItem.getBoundingClientRect();
    const itemCenterDelta = targetBounds.top + targetBounds.height / 2
      - (streamBounds.top + streamBounds.height / 2);
    const initialScrollTop = stream.scrollTop;
    const flickDistance = itemCenterDelta * 0.65;
    const startedAt = performance.now();
    let inputEndedAt = Number.POSITIVE_INFINITY;
    let firstSelectionChangedAt: number | null = null;
    let selectionChangesDuringInput = 0;
    let selectionChangesAfterInput = 0;
    const samples: Array<{ at: number; scrollTop: number }> = [];
    let sampling = true;

    const observer = new MutationObserver((records) => {
      for (const record of records) {
        if (record.attributeName !== 'aria-activedescendant') continue;
        firstSelectionChangedAt ??= performance.now();
        if (performance.now() <= inputEndedAt) selectionChangesDuringInput += 1;
        else selectionChangesAfterInput += 1;
      }
    });
    observer.observe(stream, { attributes: true, attributeFilter: ['aria-activedescendant'] });

    const sampleFrames = () => {
      samples.push({ at: performance.now(), scrollTop: stream.scrollTop });
      if (sampling) requestAnimationFrame(sampleFrames);
    };
    requestAnimationFrame(sampleFrames);

    for (let step = 1; step <= 3; step += 1) {
      stream.dispatchEvent(new WheelEvent('wheel', {
        bubbles: true,
        deltaY: flickDistance / 3,
      }));
      stream.scrollTop = initialScrollTop + flickDistance * (step / 3);
      stream.dispatchEvent(new Event('scroll'));
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    }
    inputEndedAt = performance.now();

    await new Promise<void>((resolve, reject) => {
      const checkSettled = () => {
        const activeId = stream.getAttribute('aria-activedescendant');
        const activeItem = activeId ? document.getElementById(activeId) : null;
        if (
          activeId !== initialActiveId
          && activeItem?.matches('[data-centered="true"][data-settled="true"]')
        ) {
          resolve();
          return;
        }
        if (performance.now() - startedAt > 1_000) {
          reject(new Error('Flow short touchpad flick did not settle within one second'));
          return;
        }
        requestAnimationFrame(checkSettled);
      };
      requestAnimationFrame(checkSettled);
    });

    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    sampling = false;
    observer.disconnect();
    const postInputSamples = samples.filter((sample) => sample.at >= inputEndedAt);
    let maxPostInputFrameDelta = 0;
    for (let index = 1; index < postInputSamples.length; index += 1) {
      maxPostInputFrameDelta = Math.max(
        maxPostInputFrameDelta,
        Math.abs(postInputSamples[index]!.scrollTop - postInputSamples[index - 1]!.scrollTop),
      );
    }

    return {
      elapsedMs: performance.now() - startedAt,
      firstSelectionChangeDelayMs: firstSelectionChangedAt === null
        ? Number.POSITIVE_INFINITY
        : firstSelectionChangedAt - inputEndedAt,
      itemCenterDistance: Math.abs(itemCenterDelta),
      maxPostInputFrameDelta,
      selectionChangesAfterInput,
      selectionChangesDuringInput,
    };
  });

  expect(motion.itemCenterDistance).toBeGreaterThan(120);
  expect(motion.selectionChangesDuringInput).toBe(0);
  expect(motion.firstSelectionChangeDelayMs).toBeGreaterThanOrEqual(90);
  expect(motion.selectionChangesAfterInput).toBeLessThanOrEqual(1);
  expect(motion.maxPostInputFrameDelta).toBeLessThanOrEqual(48);
  expect(motion.elapsedMs).toBeLessThanOrEqual(500);
});

test('Flow one-step wheel browsing keeps the incoming wallpaper visually available', async ({
  page,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'The one-step preview gate is desktop-only.');
  const thumbnailDataUrl = `data:image/gif;base64,${TINY_GIF.toString('base64')}`;
  let bridgePatched = false;
  await page.route('**/src/api/mockBridge.ts*', async (route) => {
    const response = await route.fetch();
    let body = await response.text();
    const needle = 'thumbnailFor: async (path) => {';
    if (body.includes(needle)) {
      body = body.replace(needle, `${needle}
        if (path.endsWith('/wallpaper-002.jpg')) {
          await new Promise((resolve) => window.setTimeout(resolve, 1_200));
        }
        return { path, thumbnail: ${JSON.stringify(thumbnailDataUrl)}, cacheHit: false };`);
      bridgePatched = true;
    }
    await route.fulfill({ response, body });
  });
  await page.route('**/mock/path/**', (route) => route.fulfill({
    body: TINY_GIF,
    contentType: 'image/gif',
    status: 200,
  }));
  await page.goto('/');
  expect(bridgePatched).toBe(true);
  await expect(page.locator('.wallpaper-grid')).toBeVisible();
  await page.getByRole('combobox', { name: 'Wallpaper type filter' }).click();
  await page.getByRole('option', { name: 'Images', exact: true }).click();
  await expect(page.locator('.wallpaper-card').first()).toHaveAttribute(
    'data-wallpaper-path',
    /\.(?:jpe?g|png|webp)$/i,
  );
  await page.getByRole('button', { name: 'Flow' }).click();
  const stream = page.locator('.flow-preview-stream');
  await stream.focus();
  await stream.press('Home');
  await expect(page.locator('.flow-preview-item[data-index="0"][data-settled="true"]'))
    .toBeVisible();
  await expect.poll(() => page.locator('.flow-preview-item[data-index="0"] img').evaluateAll(
    (images) => images.some((image) => (
      image instanceof HTMLImageElement && image.naturalWidth > 0
    )),
  )).toBe(true);

  const target = page.locator('.flow-preview-item[data-index="1"]');
  await expect(target).toBeVisible();

  const availability = await target.evaluate(async (targetItem) => {
    const streamElement = targetItem.closest<HTMLElement>('.flow-preview-stream');
    const initialItem = streamElement?.querySelector<HTMLElement>(
      '.flow-preview-item[data-centered="true"]',
    );
    if (!streamElement || !initialItem) throw new Error('Flow one-step setup is unavailable');
    const streamBounds = streamElement.getBoundingClientRect();
    const targetBounds = targetItem.getBoundingClientRect();
    const targetCenterDelta = targetBounds.top + targetBounds.height / 2
      - (streamBounds.top + streamBounds.height / 2);
    const initialScrollTop = streamElement.scrollTop;
    const startedAt = performance.now();
    let blankStartedAt: number | null = null;
    let maxBlankMs = 0;
    let firstCenteredAt: number | null = null;
    let firstReadyAfterCenteredAt: number | null = null;
    let sampling = true;

    const isVisuallyReady = () => Array.from(targetItem.querySelectorAll('img, video')).some(
      (media) => {
        const style = getComputedStyle(media);
        const bounds = media.getBoundingClientRect();
        const loaded = media instanceof HTMLImageElement
          ? media.naturalWidth > 0
          : media instanceof HTMLVideoElement && media.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA;
        return loaded
          && style.display !== 'none'
          && style.visibility !== 'hidden'
          && Number(style.opacity) > 0.01
          && bounds.width > 1
          && bounds.height > 1;
      },
    );

    const sample = (now: number) => {
      const itemBounds = targetItem.getBoundingClientRect();
      const centerDistance = Math.abs(
        itemBounds.top + itemBounds.height / 2
          - (streamBounds.top + streamBounds.height / 2),
      );
      const visuallyRelevant = centerDistance <= itemBounds.height * 0.5
        || targetItem.dataset.centered === 'true';
      if (targetItem.dataset.centered === 'true') firstCenteredAt ??= now;
      const ready = isVisuallyReady();
      if (firstCenteredAt !== null && ready) firstReadyAfterCenteredAt ??= now;
      if (visuallyRelevant && !ready) blankStartedAt ??= now;
      else if (blankStartedAt !== null) {
        maxBlankMs = Math.max(maxBlankMs, now - blankStartedAt);
        blankStartedAt = null;
      }
      if (sampling) requestAnimationFrame(sample);
    };
    requestAnimationFrame(sample);

    for (let step = 1; step <= 3; step += 1) {
      streamElement.dispatchEvent(new WheelEvent('wheel', {
        bubbles: true,
        deltaY: targetCenterDelta * 0.65 / 3,
      }));
      streamElement.scrollTop = initialScrollTop + targetCenterDelta * 0.65 * (step / 3);
      streamElement.dispatchEvent(new Event('scroll'));
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    }

    await new Promise<void>((resolve, reject) => {
      const waitForReady = () => {
        if (
          targetItem.matches('[data-centered="true"][data-settled="true"]')
          && isVisuallyReady()
        ) {
          resolve();
          return;
        }
        if (performance.now() - startedAt > 3_000) {
          reject(new Error('Flow one-step target did not become visually ready within three seconds'));
          return;
        }
        requestAnimationFrame(waitForReady);
      };
      requestAnimationFrame(waitForReady);
    });
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    sampling = false;
    if (blankStartedAt !== null) maxBlankMs = Math.max(maxBlankMs, performance.now() - blankStartedAt);

    return {
      centeredReadyDelayMs: firstCenteredAt === null || firstReadyAfterCenteredAt === null
        ? Number.POSITIVE_INFINITY
        : firstReadyAfterCenteredAt - firstCenteredAt,
      maxBlankMs,
    };
  });

  expect(availability.maxBlankMs).toBeLessThanOrEqual(50);
  expect(availability.centeredReadyDelayMs).toBeLessThanOrEqual(50);
});

test('Flow rapid arrows keep the index rail and preview synchronized', async ({
  page,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'The keyboard stress gate is desktop-only.');
  await page.goto('/');
  await expect(page.locator('.wallpaper-grid')).toBeVisible();
  await page.getByRole('button', { name: 'Flow' }).click();
  const stream = page.locator('.flow-preview-stream');
  await stream.focus();
  await stream.press('Home');
  await expect(page.locator('.flow-preview-item[data-index="0"][data-settled="true"]'))
    .toBeVisible();

  const audit = await stream.evaluate(async (streamElement) => {
    const readState = () => {
      const centered = streamElement.querySelector<HTMLElement>(
        '.flow-preview-item[data-centered="true"]',
      );
      const rail = document.querySelector<HTMLElement>('.flow-index-rail__item[data-centered]');
      const streamBounds = streamElement.getBoundingClientRect();
      const streamCenter = streamBounds.top + streamBounds.height / 2;
      const geometric = Array.from(
        streamElement.querySelectorAll<HTMLElement>('.flow-preview-item'),
      ).map((item) => {
        const bounds = item.getBoundingClientRect();
        return {
          index: Number(item.dataset.index),
          distance: Math.abs(bounds.top + bounds.height / 2 - streamCenter),
        };
      }).sort((left, right) => left.distance - right.distance)[0];
      return {
        centeredIndex: Number(centered?.dataset.index),
        centeredWallpaperId: Number(centered?.dataset.wallpaperId),
        geometricIndex: geometric?.index ?? Number.NaN,
        geometricWallpaperId: Number(
          streamElement.querySelector<HTMLElement>(
            `.flow-preview-item[data-index="${geometric?.index}"]`,
          )?.dataset.wallpaperId,
        ),
        railWallpaperId: Number(rail?.dataset.wallpaperId),
        settled: centered?.dataset.settled === 'true',
      };
    };
    const pressArrowDown = () => streamElement.dispatchEvent(new KeyboardEvent('keydown', {
      bubbles: true,
      cancelable: true,
      key: 'ArrowDown',
    }));
    const waitForState = async (
      predicate: (state: ReturnType<typeof readState>) => boolean,
      timeoutMs: number,
    ) => {
      const startedAt = performance.now();
      while (performance.now() - startedAt <= timeoutMs) {
        const state = readState();
        if (predicate(state)) return { elapsedMs: performance.now() - startedAt, state };
        await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      }
      return { elapsedMs: performance.now() - startedAt, state: readState() };
    };

    const initialIndex = readState().centeredIndex;
    const rapidPresses = 8;
    const movingRailSamples: Array<{
      expectedOrdinal: number;
      centeredOrdinal: number | null;
      visibleOrdinals: number[];
    }> = [];
    for (let index = 0; index < rapidPresses; index += 1) {
      pressArrowDown();
      await new Promise((resolve) => window.setTimeout(resolve, 12));
      const visibleOrdinals = Array.from(
        document.querySelectorAll<HTMLElement>('.flow-index-rail__ordinal'),
      ).map((ordinal) => Number(ordinal.textContent));
      const centeredOrdinalText = document.querySelector<HTMLElement>(
        '.flow-index-rail__item[data-centered] .flow-index-rail__ordinal',
      )?.textContent;
      movingRailSamples.push({
        expectedOrdinal: initialIndex + index + 2,
        centeredOrdinal: centeredOrdinalText === undefined
          ? null
          : Number(centeredOrdinalText),
        visibleOrdinals,
      });
    }
    const rapid = await waitForState(
      (state) => state.settled && state.centeredIndex !== initialIndex,
      2_000,
    );

    return {
      expectedRapidIndex: initialIndex + rapidPresses,
      movingRailSamples,
      rapid: rapid.state,
      rapidElapsedMs: rapid.elapsedMs,
    };
  });

  expect.soft(audit.rapid.centeredIndex).toBe(audit.expectedRapidIndex);
  expect.soft(audit.rapid.railWallpaperId).toBe(audit.rapid.centeredWallpaperId);
  expect.soft(audit.rapid.geometricIndex).toBe(audit.rapid.centeredIndex);
  expect.soft(audit.rapid.geometricWallpaperId).toBe(audit.rapid.centeredWallpaperId);
  expect.soft(audit.rapidElapsedMs).toBeLessThanOrEqual(600);
  for (const sample of audit.movingRailSamples) {
    expect.soft(sample.centeredOrdinal).toBe(sample.expectedOrdinal);
    expect.soft(sample.visibleOrdinals).toContain(sample.expectedOrdinal);
  }
});

test('Flow large-screen short wheel keeps moving and settle phases paced', async ({
  page,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'The large-screen frame pacing gate is desktop-only.');
  await page.setViewportSize({ width: 1_828, height: 1_142 });
  await page.goto('/');
  await expect(page.locator('.wallpaper-grid')).toBeVisible();
  await page.getByRole('button', { name: 'Flow' }).click();
  const stream = page.locator('.flow-preview-stream');
  await expect(page.locator('.flow-preview-item[data-centered="true"][data-settled="true"]'))
    .toBeVisible();
  await page.waitForTimeout(500);

  const directPositioning = await page.locator('.flow-preview-stream__virtual').evaluate(
    (container) => ({
      containerHeight: container.style.height,
      itemTransforms: Array.from(container.querySelectorAll<HTMLElement>('.flow-preview-item'))
        .map((item) => item.style.transform),
    }),
  );
  expect(directPositioning.containerHeight).toMatch(/^\d+(?:\.\d+)?px$/);
  expect(directPositioning.itemTransforms.length).toBeGreaterThan(0);
  expect(directPositioning.itemTransforms.filter((transform) => (
    !transform.startsWith('translate3d(0px, ')
  ))).toEqual([]);

  const idleP95FrameGapMs = await page.evaluate(() => new Promise<number>((resolve) => {
    const gaps: number[] = [];
    let previous = performance.now();
    const sample = (now: number) => {
      gaps.push(now - previous);
      previous = now;
      if (gaps.length < 45) {
        requestAnimationFrame(sample);
        return;
      }
      const sorted = gaps.slice(1).sort((left, right) => left - right);
      resolve(sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95))] ?? 0);
    };
    requestAnimationFrame(sample);
  }));
  await page.evaluate(() => {
    const frames: Array<{ gap: number; phase: 'moving' | 'unsettled' | 'settled' }> = [];
    const longTasks: number[] = [];
    let previous = performance.now();
    let previousScrollTop = document.querySelector<HTMLElement>('.flow-preview-stream')?.scrollTop
      ?? 0;
    let running = true;
    const observer = new PerformanceObserver((entries) => {
      for (const entry of entries.getEntries()) longTasks.push(entry.duration);
    });
    observer.observe({ type: 'longtask' });
    const sample = (now: number) => {
      const stream = document.querySelector<HTMLElement>('.flow-preview-stream');
      const scrollTop = stream?.scrollTop ?? previousScrollTop;
      const centered = stream?.querySelector<HTMLElement>('.flow-preview-item[data-centered="true"]');
      frames.push({
        gap: now - previous,
        phase: Math.abs(scrollTop - previousScrollTop) >= 0.5
          ? 'moving'
          : centered?.dataset.settled === 'true' ? 'settled' : 'unsettled',
      });
      previous = now;
      previousScrollTop = scrollTop;
      if (running) requestAnimationFrame(sample);
    };
    requestAnimationFrame(sample);
    Object.assign(window, {
      __flowLargeScreenFrameAudit: {
        frames,
        longTasks,
        stop: () => {
          running = false;
          observer.disconnect();
        },
      },
    });
  });

  const bounds = await stream.boundingBox();
  if (!bounds) throw new Error('Flow stream bounds are unavailable');
  await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
  await page.mouse.wheel(0, 420);
  await page.waitForTimeout(750);

  const pacing = await page.evaluate(() => {
    const audit = (window as typeof window & {
      __flowLargeScreenFrameAudit?: {
        frames: Array<{ gap: number; phase: 'moving' | 'unsettled' | 'settled' }>;
        longTasks: number[];
        stop: () => void;
      };
    }).__flowLargeScreenFrameAudit;
    if (!audit) throw new Error('Flow large-screen frame audit is unavailable');
    audit.stop();
    const summarize = (phase?: 'moving' | 'unsettled' | 'settled') => {
      const gaps = audit.frames.slice(1)
        .filter((frame) => phase === undefined || frame.phase === phase)
        .map((frame) => frame.gap)
        .sort((left, right) => left - right);
      const percentileIndex = Math.min(gaps.length - 1, Math.floor(gaps.length * 0.95));
      return {
        frames: gaps.length,
        maxFrameGapMs: gaps.at(-1) ?? 0,
        p95FrameGapMs: gaps[percentileIndex] ?? 0,
      };
    };
    return {
      ...summarize(),
      maxLongTaskMs: Math.max(0, ...audit.longTasks),
      moving: summarize('moving'),
      settled: summarize('settled'),
      unsettled: summarize('unsettled'),
    };
  });

  const p95FrameBudgetMs = idleP95FrameGapMs < 8 ? 6.5 : 24;
  expect.soft(pacing.frames).toBeGreaterThanOrEqual(25);
  expect.soft(pacing.moving.frames).toBeGreaterThanOrEqual(2);
  expect.soft(pacing.unsettled.frames).toBeGreaterThanOrEqual(5);
  expect.soft(pacing.moving.p95FrameGapMs).toBeLessThanOrEqual(p95FrameBudgetMs);
  expect.soft(pacing.unsettled.p95FrameGapMs).toBeLessThanOrEqual(p95FrameBudgetMs);
  expect.soft(pacing.maxLongTaskMs).toBeLessThanOrEqual(50);
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
