import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { readFile, unlink, writeFile } from 'node:fs/promises';
import test from 'node:test';

import { Children, isValidElement, type ReactElement, type ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ts from 'typescript';

import type { LibraryBrowserItemDTO } from '../api/types.ts';

async function importTsxModule(): Promise<typeof import('./WallpaperDetailsDialog.tsx')> {
  const sourceUrl = new URL('./WallpaperDetailsDialog.tsx', import.meta.url);
  const outputUrl = new URL(`./.WallpaperDetailsDialog.test-${randomUUID()}.mjs`, import.meta.url);
  const source = await readFile(sourceUrl, 'utf8');
  const output = ts.transpileModule(source, {
    compilerOptions: {
      jsx: ts.JsxEmit.ReactJSX,
      module: ts.ModuleKind.ESNext,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourceUrl.pathname,
  }).outputText;

  await writeFile(outputUrl, output, 'utf8');
  try {
    return await import(outputUrl.href);
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
    const rendered = node.type(node.props) as ReactNode;
    return [...matches, ...findElements(rendered, predicate)];
  }
  return [
    ...matches,
    ...Children.toArray(node.props.children).flatMap((child) => findElements(child, predicate)),
  ];
}

function portraitWallpaper(overrides: Partial<LibraryBrowserItemDTO> = {}): LibraryBrowserItemDTO {
  return {
    path: '/home/demo/Pictures/portrait-wallpaper.jpg',
    type: 'image',
    ext: 'jpg',
    backend: 'awww',
    size: 3_145_728,
    mtime: 1_700_000_000,
    resolution: '1080x1920',
    title: 'Portrait garden',
    wallpaperId: 17,
    favorite: true,
    author: 'Ada Artist',
    addedAt: '2026-07-14T00:00:00Z',
    sources: [
      { id: 1, displayName: 'Pictures' },
      { id: 2, displayName: 'Downloaded art' },
    ],
    ...overrides,
  };
}

function viewProps(overrides: Record<string, unknown> = {}) {
  return {
    open: true,
    wallpaper: portraitWallpaper(),
    previewSrc: 'asset://localhost/portrait-wallpaper.jpg',
    onClose: () => undefined,
    ...overrides,
  };
}

test('closed or unselected details dialog renders nothing', async () => {
  const { WallpaperDetailsDialogView } = await importTsxModule();

  assert.equal(WallpaperDetailsDialogView(viewProps({ open: false })), null);
  assert.equal(WallpaperDetailsDialogView(viewProps({ wallpaper: null })), null);
});

test('renders an accessible full-ratio portrait preview and complete metadata', async () => {
  const { WallpaperDetailsDialogView } = await importTsxModule();
  const markup = renderToStaticMarkup(WallpaperDetailsDialogView(viewProps()));

  assert.match(markup, /role="dialog"/);
  assert.match(markup, /aria-modal="true"/);
  assert.match(markup, /aria-labelledby="wallpaper-details-title"/);
  assert.match(markup, /aria-label="Close wallpaper details"/);
  assert.match(markup, /Portrait garden/);
  assert.match(markup, /Image/);
  assert.match(markup, /Pictures, Downloaded art/);
  assert.match(markup, /Ada Artist/);
  assert.ok(markup.includes('/home/demo/Pictures/portrait-wallpaper.jpg'));
  assert.ok(markup.includes('src="asset://localhost/portrait-wallpaper.jpg"'));
  assert.match(markup, /alt="Portrait garden preview"/);
  assert.match(markup, /object-fit:contain/);
  assert.doesNotMatch(markup, /object-fit:cover/);
});

test('omits unavailable optional metadata and explains a missing preview', async () => {
  const { WallpaperDetailsDialogView } = await importTsxModule();
  const markup = renderToStaticMarkup(WallpaperDetailsDialogView(viewProps({
    wallpaper: portraitWallpaper({ author: null, sources: [] }),
    previewSrc: null,
  })));

  assert.doesNotMatch(markup, /Ada Artist/);
  assert.doesNotMatch(markup, />Author</);
  assert.match(markup, /Source information unavailable/);
  assert.match(markup, /Preview unavailable/);
  assert.doesNotMatch(markup, /<img/);
});

test('Escape and the explicit close button call onClose', async () => {
  const { WallpaperDetailsDialogView } = await importTsxModule();
  const calls: string[] = [];
  const tree = WallpaperDetailsDialogView(viewProps({ onClose: () => calls.push('close') }));
  const [dialog] = findElements(tree, (element) => element.props.role === 'dialog');
  const [close] = findElements(tree, (element) => (
    element.type === 'button' && element.props['aria-label'] === 'Close wallpaper details'
  ));

  assert.ok(dialog);
  assert.ok(close);
  assert.equal(close.props.autoFocus, true);
  (dialog.props.onKeyDown as (event: { key: string }) => void)({ key: 'Enter' });
  assert.deepEqual(calls, []);
  (dialog.props.onKeyDown as (event: { key: string }) => void)({ key: 'Escape' });
  (close.props.onClick as () => void)();
  assert.deepEqual(calls, ['close', 'close']);
});
