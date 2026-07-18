import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { readFile, unlink, writeFile } from 'node:fs/promises';
import test from 'node:test';

import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ts from 'typescript';

async function importTsxModule(): Promise<Record<string, unknown>> {
  const sourceUrl = new URL('./ContextMenu.tsx', import.meta.url);
  const outputUrl = new URL(`./.ContextMenu.test-${randomUUID()}.mjs`, import.meta.url);
  const source = await readFile(sourceUrl, 'utf8');
  const output = ts.transpileModule(source, {
    compilerOptions: {
      jsx: ts.JsxEmit.ReactJSX,
      module: ts.ModuleKind.ESNext,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourceUrl.pathname,
  }).outputText.replace("from '../events/appEvents';", "from '../events/appEvents.ts';");

  await writeFile(outputUrl, output, 'utf8');
  try {
    return await import(outputUrl.href) as Record<string, unknown>;
  } finally {
    await unlink(outputUrl);
  }
}

test('renders the context actions as an accessible menu with one initial tab stop', async () => {
  const module = await importTsxModule();
  const ContextMenu = module.default as (props: Record<string, unknown>) => React.ReactNode;
  const markup = renderToStaticMarkup(createElement(ContextMenu, {
    x: 20,
    y: 40,
    path: '/wallpapers/example.jpg',
    actions: [
      { label: 'Apply', action: () => undefined },
      { label: 'Delete', action: () => undefined, danger: true },
    ],
    onClose: () => undefined,
  }));

  assert.match(markup, /role="menu"/);
  assert.match(markup, /aria-label="Wallpaper actions"/);
  assert.equal((markup.match(/role="menuitem"/g) ?? []).length, 2);
  assert.equal((markup.match(/tabindex="0"/g) ?? []).length, 1);
  assert.equal((markup.match(/tabindex="-1"/g) ?? []).length, 1);
  assert.equal((markup.match(/autofocus=""/g) ?? []).length, 1);
});

test('resolves wrapping menu navigation, direct jumps, and close keys', async () => {
  const module = await importTsxModule();
  const resolve = module.resolveContextMenuKey;
  assert.equal(typeof resolve, 'function');
  const resolveKey = resolve as (
    key: string,
    currentIndex: number,
    itemCount: number,
  ) =>
    | { type: 'focus'; index: number }
    | { type: 'close'; restoreFocus: boolean; deferUntilAfterTraversal: boolean }
    | null;

  assert.deepEqual(resolveKey('ArrowDown', -1, 3), { type: 'focus', index: 0 });
  assert.deepEqual(resolveKey('ArrowUp', -1, 3), { type: 'focus', index: 2 });
  assert.deepEqual(resolveKey('ArrowDown', 2, 3), { type: 'focus', index: 0 });
  assert.deepEqual(resolveKey('ArrowUp', 0, 3), { type: 'focus', index: 2 });
  assert.deepEqual(resolveKey('Home', 1, 3), { type: 'focus', index: 0 });
  assert.deepEqual(resolveKey('End', 1, 3), { type: 'focus', index: 2 });
  assert.deepEqual(resolveKey('Escape', 1, 3), {
    type: 'close',
    restoreFocus: true,
    deferUntilAfterTraversal: false,
  });
  assert.deepEqual(resolveKey('Tab', 1, 3), {
    type: 'close',
    restoreFocus: false,
    deferUntilAfterTraversal: true,
  });
  assert.equal(resolveKey('Enter', 1, 3), null);
  assert.equal(resolveKey('ArrowDown', -1, 0), null);
});

test('keeps a context menu inside the viewport margin', async () => {
  const module = await importTsxModule();
  const clamp = module.clampContextMenuPosition;
  assert.equal(typeof clamp, 'function');
  const clampPosition = clamp as (input: {
    x: number;
    y: number;
    menuWidth: number;
    menuHeight: number;
    viewportWidth: number;
    viewportHeight: number;
    margin?: number;
  }) => { left: number; top: number };

  assert.deepEqual(clampPosition({
    x: 100,
    y: 80,
    menuWidth: 180,
    menuHeight: 100,
    viewportWidth: 400,
    viewportHeight: 300,
  }), { left: 100, top: 80 });
  assert.deepEqual(clampPosition({
    x: 390,
    y: 290,
    menuWidth: 180,
    menuHeight: 100,
    viewportWidth: 400,
    viewportHeight: 300,
  }), { left: 212, top: 192 });
  assert.deepEqual(clampPosition({
    x: -20,
    y: -10,
    menuWidth: 180,
    menuHeight: 100,
    viewportWidth: 400,
    viewportHeight: 300,
  }), { left: 8, top: 8 });
});
