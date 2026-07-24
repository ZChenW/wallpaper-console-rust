import assert from 'node:assert/strict';
import test from 'node:test';

import * as flow from './wallpaperFlowModel.ts';

test('flow timing constants keep scroll settling and snap animation independent', () => {
  assert.equal(flow.FLOW_SCROLL_IDLE_MS, 100);
  assert.equal(flow.FLOW_SNAP_DURATION_MS, 200);
});

test('clamped flow navigation is finite and never wraps at either end', () => {
  assert.equal(flow.clampFlowIndex(2, 5), 2);
  assert.equal(flow.clampFlowIndex(-20, 5), 0);
  assert.equal(flow.clampFlowIndex(20, 5), 4);
  assert.equal(flow.clampFlowIndex(0, 0), null);

  assert.deepEqual(flow.resolveFlowKey({
    key: 'ArrowUp',
    currentIndex: 0,
    itemCount: 5,
  }), { type: 'navigate', index: 0, requestLoadMore: false });
  assert.deepEqual(flow.resolveFlowKey({
    key: 'ArrowDown',
    currentIndex: 4,
    itemCount: 5,
  }), { type: 'navigate', index: 4, requestLoadMore: false });
});

test('flow keys resolve vertical, page, boundary, selection, apply, and context intents', () => {
  const navigationCases = [
    ['ArrowUp', 4, 3],
    ['ArrowDown', 4, 5],
    ['PageUp', 4, 1],
    ['PageDown', 4, 7],
    ['Home', 4, 0],
  ] as const;

  for (const [key, currentIndex, expectedIndex] of navigationCases) {
    assert.deepEqual(flow.resolveFlowKey({
      key,
      currentIndex,
      itemCount: 9,
      pageStep: 3,
    }), { type: 'navigate', index: expectedIndex, requestLoadMore: false });
  }

  assert.deepEqual(flow.resolveFlowKey({
    key: 'Enter',
    currentIndex: 4,
    itemCount: 9,
  }), { type: 'select', index: 4 });
  assert.deepEqual(flow.resolveFlowKey({
    key: 'Enter',
    currentIndex: 4,
    itemCount: 9,
    ctrlKey: true,
  }), { type: 'apply', index: 4 });
  assert.deepEqual(flow.resolveFlowKey({
    key: 'Enter',
    currentIndex: 4,
    itemCount: 9,
    metaKey: true,
  }), { type: 'apply', index: 4 });
  assert.deepEqual(flow.resolveFlowKey({
    key: 'F10',
    currentIndex: 4,
    itemCount: 9,
    shiftKey: true,
  }), { type: 'context', index: 4 });
});

test('End moves to the loaded tail and obeys the caller-owned session gate', () => {
  assert.deepEqual(flow.resolveFlowKey({
    key: 'End',
    currentIndex: 1,
    itemCount: 4,
    hasMore: true,
    loadingMore: false,
    endLoadRequestAllowed: true,
  }), { type: 'navigate', index: 3, requestLoadMore: true });
  assert.deepEqual(flow.resolveFlowKey({
    key: 'End',
    currentIndex: 1,
    itemCount: 4,
    hasMore: true,
    loadingMore: false,
    endLoadRequestAllowed: false,
  }), { type: 'navigate', index: 3, requestLoadMore: false });
  assert.deepEqual(flow.resolveFlowKey({
    key: 'End',
    currentIndex: 3,
    itemCount: 6,
    hasMore: true,
    loadingMore: false,
    endLoadRequestAllowed: true,
  }), { type: 'navigate', index: 5, requestLoadMore: true });
  assert.deepEqual(flow.resolveFlowKey({
    key: 'End',
    currentIndex: 1,
    itemCount: 4,
    hasMore: true,
    loadingMore: true,
    endLoadRequestAllowed: true,
  }), { type: 'navigate', index: 3, requestLoadMore: false });
  assert.deepEqual(flow.resolveFlowKey({
    key: 'End',
    currentIndex: 1,
    itemCount: 4,
    hasMore: false,
    endLoadRequestAllowed: true,
  }), { type: 'navigate', index: 3, requestLoadMore: false });
});

test('empty flow and unhandled keys produce no intent', () => {
  for (const key of ['ArrowDown', 'End', 'Enter', 'F10']) {
    assert.equal(flow.resolveFlowKey({
      key,
      currentIndex: 0,
      itemCount: 0,
      shiftKey: true,
      ctrlKey: true,
      hasMore: true,
    }), null);
  }

  for (const key of ['Tab', ' ', 'Space', 'F10', 'Escape', 'ArrowLeft']) {
    assert.equal(flow.resolveFlowKey({
      key,
      currentIndex: 2,
      itemCount: 5,
    }), null);
  }
});

test('nearest flow item compares geometric centers rather than array positions', () => {
  assert.equal(flow.nearestFlowCenterIndex({
    items: [
      { id: 'first', index: 4, start: 20, size: 100 },
      { id: 'near', index: 9, start: 140, size: 80 },
      { id: 'far', index: 12, start: 230, size: 80 },
    ],
    viewportStart: 100,
    viewportSize: 200,
  }), 9);
});

test('nearest flow item keeps the previous ID through an exact center tie', () => {
  const items = [
    { id: 'before', index: 3, start: 130, size: 100 },
    { id: 'after', index: 4, start: 170, size: 100 },
  ];

  assert.equal(flow.nearestFlowCenterIndex({
    items,
    viewportStart: 100,
    viewportSize: 200,
    previousCenteredId: 'after',
  }), 4);
  assert.equal(flow.nearestFlowCenterIndex({
    items,
    viewportStart: 100,
    viewportSize: 200,
  }), 3);
  assert.equal(flow.nearestFlowCenterIndex({
    items: [],
    viewportStart: 100,
    viewportSize: 200,
    previousCenteredId: 'after',
  }), null);
});

test('local flow window includes at most seven loaded items on each side', () => {
  assert.deepEqual(flow.localFlowIndexWindow(12, 30), {
    startIndex: 5,
    endIndex: 19,
  });
  assert.deepEqual(flow.localFlowIndexWindow(1, 30), {
    startIndex: 0,
    endIndex: 8,
  });
  assert.deepEqual(flow.localFlowIndexWindow(29, 30), {
    startIndex: 22,
    endIndex: 29,
  });
  assert.equal(flow.localFlowIndexWindow(0, 0), null);
});

test('virtual flow range retains one offscreen active option in index order', () => {
  const visibleIndexes = [114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124];

  const retainedIndexes = flow.retainFlowActiveIndex(visibleIndexes, 0, 125);

  assert.deepEqual(retainedIndexes, [0, ...visibleIndexes]);
  assert.equal(retainedIndexes.length, visibleIndexes.length + 1);
});

test('virtual flow range stays unchanged for rendered or invalid active indexes', () => {
  const visibleIndexes = [4, 5, 6];

  assert.deepEqual(flow.retainFlowActiveIndex(visibleIndexes, 5, 10), visibleIndexes);
  assert.deepEqual(flow.retainFlowActiveIndex(visibleIndexes, -1, 10), visibleIndexes);
  assert.deepEqual(flow.retainFlowActiveIndex(visibleIndexes, 10, 10), visibleIndexes);
});

test('local index alignment offset pins a boundary entry to the rail center', () => {
  assert.equal(flow.flowIndexAlignmentOffset({
    railStart: 100,
    railSize: 600,
    itemStart: 180,
    itemSize: 30,
  }), 205);
  assert.equal(flow.flowIndexAlignmentOffset({
    railStart: 100,
    railSize: 600,
    itemStart: 500,
    itemSize: 30,
  }), -115);
  assert.equal(flow.flowIndexAlignmentOffset({
    railStart: Number.NaN,
    railSize: 600,
    itemStart: 500,
    itemSize: 30,
  }), 0);
});

test('local index alignment accumulates from the current transform without resetting layout', () => {
  assert.equal(flow.nextFlowIndexAlignmentOffset(80, {
    railStart: 100,
    railSize: 600,
    itemStart: 500,
    itemSize: 30,
  }), -35);
  assert.equal(flow.nextFlowIndexAlignmentOffset(-80, {
    railStart: 100,
    railSize: 600,
    itemStart: 180,
    itemSize: 30,
  }), 125);
  assert.equal(flow.nextFlowIndexAlignmentOffset(Number.NaN, {
    railStart: 100,
    railSize: 600,
    itemStart: 180,
    itemSize: 30,
  }), 205);
});

test('initial flow anchor restarts at the loaded current ID or falls back to first', () => {
  const ids = ['first', 'current', 'last'];
  assert.deepEqual(flow.resolveInitialFlowAnchor(ids, 'current'), {
    id: 'current',
    index: 1,
  });
  assert.deepEqual(flow.resolveInitialFlowAnchor(ids, 'missing'), {
    id: 'first',
    index: 0,
  });
  assert.deepEqual(flow.resolveInitialFlowAnchor(ids, null), {
    id: 'first',
    index: 0,
  });
  assert.equal(flow.resolveInitialFlowAnchor([], 'missing'), null);
});

test('mode switch anchor prefers loaded selection, then outgoing ID, then first', () => {
  const ids = ['first', 'outgoing', 'selected'];
  assert.deepEqual(flow.resolveModeSwitchFlowAnchor(ids, 'selected', 'outgoing'), {
    id: 'selected',
    index: 2,
  });
  assert.deepEqual(flow.resolveModeSwitchFlowAnchor(ids, 'missing', 'outgoing'), {
    id: 'outgoing',
    index: 1,
  });
  assert.deepEqual(flow.resolveModeSwitchFlowAnchor(ids, 'missing', 'also-missing'), {
    id: 'first',
    index: 0,
  });
  assert.equal(flow.resolveModeSwitchFlowAnchor([], 'selected', 'outgoing'), null);
});

test('query reset anchor always returns the first loaded item', () => {
  assert.deepEqual(flow.resolveQueryResetFlowAnchor(['first', 'second']), {
    id: 'first',
    index: 0,
  });
  assert.equal(flow.resolveQueryResetFlowAnchor([]), null);
});

test('estimated flow item height follows aspect ratio and explicit bounds', () => {
  assert.equal(flow.estimateFlowItemSize({
    availableWidth: 320,
    aspectRatio: 16 / 9,
  }), 180);
  assert.equal(flow.estimateFlowItemSize({
    availableWidth: 180,
    aspectRatio: 0.5,
  }), 360);
  assert.equal(flow.estimateFlowItemSize({
    availableWidth: 320,
    aspectRatio: 16 / 9,
    minHeight: 200,
  }), 200);
  assert.equal(flow.estimateFlowItemSize({
    availableWidth: 180,
    aspectRatio: 0.5,
    maxHeight: 300,
  }), 300);
});

test('flow page step uses complete estimated items and stays usable for invalid geometry', () => {
  assert.equal(flow.flowPageStep({
    viewportSize: 640,
    itemSize: 180,
    gap: 20,
  }), 3);
  assert.equal(flow.flowPageStep({
    viewportSize: 100,
    itemSize: 180,
    gap: 20,
  }), 1);
  assert.equal(flow.flowPageStep({
    viewportSize: Number.NaN,
    itemSize: 0,
    gap: -1,
  }), 1);
});

test('visible flow thumbnail range follows viewport geometry with bounded overscan', () => {
  assert.deepEqual(flow.visibleFlowThumbnailRange({
    itemCount: 20,
    scrollOffset: 210,
    viewportSize: 380,
    itemSize: 180,
    gap: 20,
  }), { startIndex: 1, endIndex: 2 });
  assert.deepEqual(flow.visibleFlowThumbnailRange({
    itemCount: 20,
    scrollOffset: 210,
    viewportSize: 380,
    itemSize: 180,
    gap: 20,
    overscan: 1,
  }), { startIndex: 0, endIndex: 3 });
  assert.deepEqual(flow.visibleFlowThumbnailRange({
    itemCount: 10,
    scrollOffset: 1_800,
    viewportSize: 400,
    itemSize: 180,
    gap: 20,
    overscan: 2,
  }), { startIndex: 7, endIndex: 9 });
  assert.equal(flow.visibleFlowThumbnailRange({
    itemCount: 0,
    scrollOffset: 0,
    viewportSize: 400,
    itemSize: 180,
  }), null);
});

test('measured thumbnail range intersects each heterogeneous item geometry', () => {
  const items = [
    { id: 'short-before', index: 1, start: 300, size: 100 },
    { id: 'tall-visible', index: 2, start: 50, size: 400 },
    { id: 'short-visible', index: 3, start: 470, size: 20 },
    { id: 'after', index: 4, start: 500, size: 200 },
  ];

  assert.deepEqual(flow.visibleMeasuredFlowThumbnailRange({
    items,
    itemCount: 6,
    viewportStart: 400,
    viewportSize: 100,
  }), { startIndex: 2, endIndex: 3 });
  assert.deepEqual(flow.visibleMeasuredFlowThumbnailRange({
    items,
    itemCount: 6,
    viewportStart: 400,
    viewportSize: 100,
    overscan: 1,
  }), { startIndex: 1, endIndex: 4 });
  assert.equal(flow.visibleMeasuredFlowThumbnailRange({
    items,
    itemCount: 6,
    viewportStart: 700,
    viewportSize: 100,
  }), null);
  assert.equal(flow.visibleMeasuredFlowThumbnailRange({
    items: [],
    itemCount: 0,
    viewportStart: 0,
    viewportSize: 100,
  }), null);
});

test('near-tail paging requests one next page only when eligible', () => {
  assert.equal(flow.shouldRequestFlowNextPage({
    itemCount: 30,
    visibleEndIndex: 26,
    threshold: 3,
    hasMore: true,
    loadingMore: false,
    automaticAppendPaused: false,
  }), true);
  assert.equal(flow.shouldRequestFlowNextPage({
    itemCount: 30,
    visibleEndIndex: 25,
    threshold: 3,
    hasMore: true,
    loadingMore: false,
    automaticAppendPaused: false,
  }), false);
  assert.equal(flow.shouldRequestFlowNextPage({
    itemCount: 30,
    visibleEndIndex: 29,
    hasMore: false,
    loadingMore: false,
    automaticAppendPaused: false,
  }), false);
  assert.equal(flow.shouldRequestFlowNextPage({
    itemCount: 30,
    visibleEndIndex: 29,
    hasMore: true,
    loadingMore: true,
    automaticAppendPaused: false,
  }), false);
  assert.equal(flow.shouldRequestFlowNextPage({
    itemCount: 30,
    visibleEndIndex: 29,
    hasMore: true,
    loadingMore: false,
    refreshing: true,
    automaticAppendPaused: false,
  }), false);
  assert.equal(flow.shouldRequestFlowNextPage({
    itemCount: 0,
    visibleEndIndex: null,
    hasMore: true,
    loadingMore: false,
    automaticAppendPaused: false,
  }), false);
});

test('automatic append pause blocks an otherwise eligible near-tail request', () => {
  assert.equal(flow.shouldRequestFlowNextPage({
    itemCount: 30,
    visibleEndIndex: 29,
    hasMore: true,
    loadingMore: false,
    automaticAppendPaused: true,
  }), false);
});

test('flow scrolling becomes immediate when reduced motion is requested', () => {
  assert.equal(flow.flowScrollBehavior(false), 'smooth');
  assert.equal(flow.flowScrollBehavior(true), 'auto');
});

test('direct rail activation is immediate without disabling other smooth navigation', () => {
  assert.equal(flow.flowScrollBehavior(false, true), 'auto');
  assert.equal(flow.flowScrollBehavior(false, false), 'smooth');
});

test('enhanced flow media requires every active settled selection gate', () => {
  const eligible = {
    mediaType: 'image',
    active: true,
    settled: true,
    centered: true,
    selected: true,
    reducedMotion: false,
  } as const;

  for (const mediaType of ['image', 'gif', 'video']) {
    assert.equal(flow.isEnhancedFlowMediaEligible({ ...eligible, mediaType }), true);
  }

  for (const ineligible of [
    { ...eligible, active: false },
    { ...eligible, settled: false },
    { ...eligible, centered: false },
    { ...eligible, selected: false },
    { ...eligible, reducedMotion: true },
  ]) {
    assert.equal(flow.isEnhancedFlowMediaEligible(ineligible), false);
  }
});

test('Wallpaper Engine scenes and unsupported media stay static in Flow', () => {
  const state = {
    active: true,
    settled: true,
    centered: true,
    selected: true,
    reducedMotion: false,
  } as const;
  assert.equal(flow.isEnhancedFlowMediaEligible({ ...state, mediaType: 'we_scene' }), false);
  assert.equal(flow.isEnhancedFlowMediaEligible({ ...state, mediaType: 'we_web' }), false);
  assert.equal(flow.isEnhancedFlowMediaEligible({ ...state, mediaType: 'unsupported' }), false);
});

test('flow state presentation adds every interaction class and attribute independently', () => {
  assert.deepEqual(flow.flowStatePresentation('wallpaper-flow__item custom', {
    active: true,
    settled: true,
    centered: true,
    hovered: true,
    selected: true,
    current: true,
    applying: true,
    pending: true,
    favorite: true,
  }), {
    className: [
      'wallpaper-flow__item custom',
      'is-active',
      'is-settled',
      'is-centered',
      'is-hovered',
      'is-selected',
      'is-current',
      'is-applying',
      'is-pending',
      'is-favorite',
    ].join(' '),
    attributes: {
      'data-active': true,
      'data-settled': true,
      'data-centered': true,
      'data-hovered': true,
      'data-selected': true,
      'data-current': true,
      'data-applying': true,
      'data-pending': true,
      'data-favorite': true,
    },
  });
  assert.deepEqual(flow.flowStatePresentation('wallpaper-flow__item', {
    active: false,
    settled: false,
    centered: true,
    hovered: false,
    selected: false,
    current: true,
    applying: false,
    pending: true,
    favorite: false,
  }), {
    className: 'wallpaper-flow__item is-centered is-current is-pending',
    attributes: {
      'data-active': undefined,
      'data-settled': undefined,
      'data-centered': true,
      'data-hovered': undefined,
      'data-selected': undefined,
      'data-current': true,
      'data-applying': undefined,
      'data-pending': true,
      'data-favorite': undefined,
    },
  });
});

test('visible Flow state labels are ordered and separator-ready without concatenation', () => {
  assert.deepEqual(flow.flowStateLabels({
    selected: true,
    current: true,
    applying: true,
    pending: true,
    favorite: true,
  }), ['Selected', 'Current', 'Applying', 'Pending', 'Favorite']);
  assert.equal(flow.flowStateLabels({ favorite: true }).join(' · '), 'Favorite');
  assert.deepEqual(flow.flowStateLabels({}), []);
});

test('scroll and hover helpers produce passive state only, never select or apply', () => {
  const scrollIntent = flow.resolveFlowScrollIntent(2, 5);
  const hoverIntent = flow.resolveFlowHoverIntent(3, 5);

  assert.deepEqual(scrollIntent, { type: 'center', index: 2 });
  assert.deepEqual(hoverIntent, { type: 'hover', index: 3 });
  for (const intent of [scrollIntent, hoverIntent]) {
    assert.ok(intent);
    assert.equal('select' in intent, false);
    assert.equal('apply' in intent, false);
  }

  assert.equal(flow.resolveFlowScrollIntent(null, 5), null);
  assert.equal(flow.resolveFlowScrollIntent(0, 0), null);
  assert.equal(flow.resolveFlowHoverIntent(-1, 5), null);
  assert.equal(flow.resolveFlowHoverIntent(undefined, 5), null);
});
