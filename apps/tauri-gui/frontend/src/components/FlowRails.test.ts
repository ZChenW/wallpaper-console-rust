import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { readFile, unlink, writeFile } from 'node:fs/promises';
import test from 'node:test';

import {
  Children,
  isValidElement,
  type ReactElement,
  type ReactNode,
} from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ts from 'typescript';

import type { LibraryBrowserItemDTO } from '../api/types.ts';

async function importTsxModule<T>(fileName: string): Promise<T> {
  const sourceUrl = new URL(`./${fileName}`, import.meta.url);
  const outputUrl = new URL(`./.${fileName}.test-${randomUUID()}.mjs`, import.meta.url);
  const output = ts.transpileModule(await readFile(sourceUrl, 'utf8'), {
    compilerOptions: {
      jsx: ts.JsxEmit.ReactJSX,
      module: ts.ModuleKind.ESNext,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourceUrl.pathname,
  }).outputText;
  await writeFile(outputUrl, output, 'utf8');
  try {
    return await import(outputUrl.href) as T;
  } finally {
    await unlink(outputUrl);
  }
}

function findElements(
  node: ReactNode,
  predicate: (element: ReactElement<Record<string, unknown>>) => boolean,
): ReactElement<Record<string, unknown>>[] {
  if (!isValidElement<Record<string, unknown>>(node)) return [];

  const matches = predicate(node) ? [node] : [];
  if (typeof node.type === 'function') {
    return [...matches, ...findElements(node.type(node.props) as ReactNode, predicate)];
  }
  return [
    ...matches,
    ...Children.toArray(node.props.children).flatMap((child) => findElements(child, predicate)),
  ];
}

function wallpaper(
  wallpaperId: number,
  title: string,
  overrides: Partial<LibraryBrowserItemDTO> = {},
): LibraryBrowserItemDTO {
  return {
    path: `/walls/${wallpaperId}.jpg`,
    type: 'image',
    ext: 'jpg',
    backend: 'awww',
    size: 12 * 1024 * 1024,
    mtime: 1_700_000_000,
    resolution: '3840x2160',
    title,
    wallpaperId,
    favorite: false,
    author: 'Ada Artist',
    addedAt: '2026-07-14T10:30:00Z',
    sources: [{ id: 1, displayName: 'Workshop' }],
    workshopId: '123456',
    rendererCompatibility: 'Full renderer support',
    ...overrides,
  };
}

test('index rail exposes exact progress and visible additive persistent state text', async () => {
  const { FlowIndexRailView } = await importTsxModule<
    typeof import('./FlowIndexRail.tsx')
  >('FlowIndexRail.tsx');
  const first = wallpaper(11, 'Night Drive', { favorite: true });
  const second = wallpaper(12, 'Quiet Lake');
  const tree = FlowIndexRailView({
    entries: [
      { entry: first, index: 8, selected: true, current: true, favorite: true },
      { entry: second, index: 9, selected: false, current: false, favorite: false },
    ],
    centeredWallpaperId: 11,
    loadedCount: 24,
    totalKnown: true,
    total: 72,
    onActivate: () => undefined,
    onHover: () => undefined,
    onOpenIndex: () => undefined,
  });
  const markup = renderToStaticMarkup(tree);

  assert.match(markup, /<nav[^>]+aria-label="Loaded wallpaper index"/);
  assert.match(markup, /24 loaded \/ 72 total/);
  assert.match(markup, /type="button"[^>]*>Index</);
  assert.match(markup, /Night Drive/);
  assert.match(markup, /Quiet Lake/);
  assert.match(markup, /Selected/);
  assert.match(markup, /Current/);
  assert.match(markup, /Favorite/);
  assert.match(markup, /data-centered="true"/);
  assert.doesNotMatch(markup, />Apply</);
});

test('index rail reserves aria-current for the runtime wallpaper instead of the centered wallpaper', async () => {
  const { FlowIndexRailView } = await importTsxModule<
    typeof import('./FlowIndexRail.tsx')
  >('FlowIndexRail.tsx');
  const centered = wallpaper(13, 'Centered only');
  const current = wallpaper(14, 'Runtime current');
  const tree = FlowIndexRailView({
    entries: [
      { entry: centered, index: 0, selected: false, current: false, favorite: false },
      { entry: current, index: 1, selected: false, current: true, favorite: false },
    ],
    centeredWallpaperId: centered.wallpaperId,
    loadedCount: 2,
    totalKnown: true,
    total: 2,
    onActivate: () => undefined,
    onHover: () => undefined,
    onOpenIndex: () => undefined,
  });
  const [centeredButton] = findElements(
    tree,
    (element) => element.props['data-wallpaper-id'] === centered.wallpaperId,
  );
  const [currentButton] = findElements(
    tree,
    (element) => element.props['data-wallpaper-id'] === current.wallpaperId,
  );

  assert.equal(centeredButton.props['aria-current'], undefined);
  assert.equal(currentButton.props['aria-current'], 'true');
});

test('index trigger identifies its dialog and exposes the centered ordinal at narrow widths', async () => {
  const { FlowIndexRailView } = await importTsxModule<
    typeof import('./FlowIndexRail.tsx')
  >('FlowIndexRail.tsx');
  const entry = wallpaper(15, 'Ninth wallpaper');
  const tree = FlowIndexRailView({
    entries: [{ entry, index: 8, selected: false, current: false, favorite: false }],
    centeredWallpaperId: entry.wallpaperId,
    loadedCount: 24,
    totalKnown: true,
    total: 72,
    onActivate: () => undefined,
    onHover: () => undefined,
    onOpenIndex: () => undefined,
  });
  const [indexButton] = findElements(
    tree,
    (element) => element.props['data-flow-index-open'] === true,
  );
  const markup = renderToStaticMarkup(tree);

  assert.equal(indexButton.props['aria-haspopup'], 'dialog');
  assert.equal(indexButton.props['aria-label'], 'Index, wallpaper 9 of 24');
  assert.match(markup, /flow-index-rail__position[^>]*>9 \/ 24</);
});

test('index rail native buttons emit activation and pointer hover without treating keyboard focus as hover', async () => {
  const { FlowIndexRailView } = await importTsxModule<
    typeof import('./FlowIndexRail.tsx')
  >('FlowIndexRail.tsx');
  const entry = wallpaper(17, 'Cloud Study');
  const activated: number[] = [];
  const hovered: Array<number | null> = [];
  let opened = 0;
  const tree = FlowIndexRailView({
    entries: [{ entry, index: 0, selected: false, current: false, favorite: false }],
    centeredWallpaperId: 17,
    loadedCount: 1,
    totalKnown: false,
    total: null,
    onActivate: (target) => activated.push(target.wallpaperId),
    onHover: (wallpaperId) => hovered.push(wallpaperId),
    onOpenIndex: () => { opened += 1; },
  });
  const [indexButton] = findElements(tree, (element) => element.props['data-flow-index-open'] === true);
  const [nameButton] = findElements(tree, (element) => element.props['data-wallpaper-id'] === 17);

  assert.equal(indexButton.type, 'button');
  assert.equal(nameButton.type, 'button');
  assert.equal(nameButton.props.onFocus, undefined);
  assert.equal(nameButton.props.onBlur, undefined);
  (indexButton.props.onClick as () => void)();
  (nameButton.props.onMouseEnter as () => void)();
  (nameButton.props.onMouseLeave as () => void)();
  (nameButton.props.onClick as () => void)();

  assert.equal(opened, 1);
  assert.deepEqual(hovered, [17, null]);
  assert.deepEqual(activated, [17]);
  assert.match(renderToStaticMarkup(tree), /1 loaded/);
  assert.doesNotMatch(renderToStaticMarkup(tree), /total/);
});

test('metadata rail presents complete glanceable metadata while omitting absent optional rows', async () => {
  const { FlowMetadataRailView } = await importTsxModule<
    typeof import('./FlowMetadataRail.tsx')
  >('FlowMetadataRail.tsx');
  const entry = wallpaper(23, 'Orbital Bloom');
  const markup = renderToStaticMarkup(FlowMetadataRailView({
    centeredEntry: entry,
    centeredIndex: 4,
    loadedCount: 24,
    totalKnown: true,
    total: 72,
    selected: true,
    current: true,
    applying: true,
    pending: true,
    favorite: true,
    favoritePending: false,
    applyAvailable: true,
    applyDisabledReason: null,
    activeQueueName: 'Orbital Bloom',
    pendingQueueName: 'After Rain',
    allViewed: false,
    showReturnToTop: false,
    onApply: () => undefined,
    onFavorite: () => undefined,
    onDetails: () => undefined,
    onReturnToTop: () => undefined,
  }));

  assert.match(markup, /Orbital Bloom/);
  assert.match(markup, /5 of 72/);
  assert.match(markup, />Source</);
  assert.match(markup, /Workshop/);
  assert.match(markup, />Type</);
  assert.match(markup, />Image</);
  assert.match(markup, /3840 × 2160/);
  assert.match(markup, /12\.0 MB/);
  assert.match(markup, /Jul 14, 2026/);
  assert.match(markup, /Ada Artist/);
  assert.match(markup, /123456/);
  assert.match(markup, />Backend</);
  assert.match(markup, /awww/);
  assert.match(markup, /Full renderer support/);
  assert.match(markup, /Selected/);
  assert.match(markup, /Current/);
  assert.match(markup, /Applying/);
  assert.match(markup, /Pending/);
  assert.match(markup, /Favorite/);
  assert.match(markup, /Applying now[^]*Orbital Bloom/);
  assert.match(markup, /Queued next[^]*After Rain/);

  const sparse = renderToStaticMarkup(FlowMetadataRailView({
    centeredEntry: wallpaper(24, 'Bare', {
      sources: [],
      resolution: 'unknown',
      size: Number.NaN,
      addedAt: '',
      author: null,
      workshopId: undefined,
      rendererCompatibility: undefined,
      backend: ' ',
    }),
    centeredIndex: 0,
    loadedCount: 1,
    totalKnown: false,
    total: null,
    selected: false,
    current: false,
    applying: false,
    pending: false,
    favorite: false,
    favoritePending: false,
    applyAvailable: true,
    applyDisabledReason: null,
    activeQueueName: null,
    pendingQueueName: null,
    allViewed: false,
    showReturnToTop: false,
    onApply: () => undefined,
    onFavorite: () => undefined,
    onDetails: () => undefined,
    onReturnToTop: () => undefined,
  }));
  assert.doesNotMatch(sparse, />Source</);
  assert.doesNotMatch(sparse, />Resolution</);
  assert.doesNotMatch(sparse, />Size</);
  assert.doesNotMatch(sparse, />Added</);
  assert.doesNotMatch(sparse, />Author</);
  assert.doesNotMatch(sparse, />Workshop</);
  assert.doesNotMatch(sparse, />Compatibility</);
  assert.doesNotMatch(sparse, />Backend</);
});

test('metadata never reports an inexact total as exact', async () => {
  const { FlowMetadataRailView } = await importTsxModule<
    typeof import('./FlowMetadataRail.tsx')
  >('FlowMetadataRail.tsx');
  const markup = renderToStaticMarkup(FlowMetadataRailView({
    centeredEntry: wallpaper(29, 'Unknown Total'),
    centeredIndex: 0,
    loadedCount: 24,
    totalKnown: false,
    total: 72,
    selected: false,
    current: false,
    applying: false,
    pending: false,
    favorite: false,
    hovered: false,
    favoritePending: false,
    applyAvailable: true,
    applyDisabledReason: null,
    activeQueueName: null,
    pendingQueueName: null,
    allViewed: true,
    showReturnToTop: false,
    onApply: () => undefined,
    onFavorite: () => undefined,
    onDetails: () => undefined,
    onReturnToTop: () => undefined,
  }));
  assert.match(markup, /24 loaded/);
  assert.match(markup, /All 24 wallpapers viewed/);
  assert.doesNotMatch(markup, /72 total/);
  assert.doesNotMatch(markup, /1 of 72/);
  assert.doesNotMatch(markup, /All 72 wallpapers viewed/);
});

test('metadata actions are visible native controls and invoke only their centered callbacks', async () => {
  const { FlowMetadataRailView } = await importTsxModule<
    typeof import('./FlowMetadataRail.tsx')
  >('FlowMetadataRail.tsx');
  const entry = wallpaper(31, 'Paper Moon');
  const actions: string[] = [];
  const tree = FlowMetadataRailView({
    centeredEntry: entry,
    centeredIndex: 0,
    loadedCount: 1,
    totalKnown: true,
    total: 1,
    selected: false,
    current: false,
    applying: false,
    pending: false,
    favorite: false,
    hovered: false,
    favoritePending: false,
    applyAvailable: true,
    applyDisabledReason: null,
    activeQueueName: null,
    pendingQueueName: null,
    allViewed: true,
    showReturnToTop: true,
    onApply: (target) => actions.push(`apply:${target.wallpaperId}`),
    onFavorite: (target) => actions.push(`favorite:${target.wallpaperId}`),
    onDetails: (target) => actions.push(`details:${target.wallpaperId}`),
    onReturnToTop: () => actions.push('top'),
  });
  const buttons = findElements(tree, (element) => element.type === 'button');
  const byAction = (action: string) => buttons.find((button) => button.props['data-flow-action'] === action);

  assert.equal(buttons.length, 4);
  assert.equal(byAction('apply')?.props.disabled, false);
  assert.equal(byAction('favorite')?.props['aria-pressed'], false);
  assert.equal(byAction('return')?.props['aria-label'], 'Return to first wallpaper');
  let stopped = 0;
  const event = { stopPropagation: () => { stopped += 1; } };
  (byAction('apply')?.props.onClick as (event: unknown) => void)(event);
  (byAction('favorite')?.props.onClick as (event: unknown) => void)(event);
  (byAction('details')?.props.onClick as (event: unknown) => void)(event);
  (byAction('return')?.props.onClick as (event: unknown) => void)(event);

  assert.deepEqual(actions, ['apply:31', 'favorite:31', 'details:31', 'top']);
  assert.equal(stopped, 4);
  const markup = renderToStaticMarkup(tree);
  assert.match(markup, /All 1 wallpapers viewed/);
  assert.match(markup, />Apply</);
  assert.match(markup, />Favorite</);
  assert.match(markup, />Details</);
});

test('metadata rail disables unavailable apply with an accessible reason and preserves a pending favorite state', async () => {
  const { FlowMetadataRailView } = await importTsxModule<
    typeof import('./FlowMetadataRail.tsx')
  >('FlowMetadataRail.tsx');
  const entry = wallpaper(41, 'Unsupported Scene');
  const tree = FlowMetadataRailView({
    centeredEntry: entry,
    centeredIndex: 0,
    loadedCount: 7,
    totalKnown: false,
    total: null,
    selected: false,
    current: false,
    applying: false,
    pending: false,
    favorite: true,
    hovered: false,
    favoritePending: true,
    applyAvailable: false,
    applyDisabledReason: 'Compatible renderer unavailable',
    activeQueueName: null,
    pendingQueueName: null,
    allViewed: true,
    showReturnToTop: false,
    onApply: () => assert.fail('disabled Apply must not invoke'),
    onFavorite: () => assert.fail('pending Favorite must not invoke'),
    onDetails: () => undefined,
    onReturnToTop: () => undefined,
  });
  const [apply] = findElements(tree, (element) => element.props['data-flow-action'] === 'apply');
  const [favorite] = findElements(tree, (element) => element.props['data-flow-action'] === 'favorite');
  const markup = renderToStaticMarkup(tree);

  assert.equal(apply.props.disabled, true);
  assert.equal(favorite.props.disabled, undefined);
  assert.equal(favorite.props['aria-disabled'], true);
  assert.equal(favorite.props['aria-busy'], true);
  assert.match(markup, /Compatible renderer unavailable/);
  assert.match(markup, /7 loaded/);
  assert.match(markup, /All 7 wallpapers viewed/);
  assert.doesNotMatch(markup, /Return to first wallpaper/);
});
