import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { readFile, unlink, writeFile } from 'node:fs/promises';
import test from 'node:test';

import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ts from 'typescript';

async function importSwitch(): Promise<typeof import('./LibraryViewSwitch.tsx')> {
  const sourceUrl = new URL('./LibraryViewSwitch.tsx', import.meta.url);
  const outputUrl = new URL(`./.LibraryViewSwitch.test-${randomUUID()}.mjs`, import.meta.url);
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
    return await import(outputUrl.href);
  } finally {
    await unlink(outputUrl);
  }
}

test('Library view switch is a labelled two-button pressed group', async () => {
  const { default: LibraryViewSwitch } = await importSwitch();
  const markup = renderToStaticMarkup(createElement(LibraryViewSwitch, {
    value: 'flow',
    onChange: () => undefined,
  }));

  assert.match(markup, /role="group"/);
  assert.match(markup, /aria-label="Library view"/);
  assert.equal((markup.match(/type="button"/g) ?? []).length, 2);
  assert.match(markup, />Grid<\/button>/);
  assert.match(markup, /aria-pressed="true"[^>]*>Flow<\/button>/);
  assert.match(markup, /aria-pressed="false"[^>]*>Grid<\/button>/);
});

test('Library view switch emits only a genuinely different mode', async () => {
  const { LibraryViewSwitchView } = await importSwitch();
  const changes: string[] = [];
  const tree = LibraryViewSwitchView({
    value: 'grid',
    onChange: (mode) => changes.push(mode),
  });
  const children = Array.isArray(tree.props.children)
    ? tree.props.children
    : [tree.props.children];
  const [grid, flow] = children;

  grid.props.onClick();
  flow.props.onClick();

  assert.deepEqual(changes, ['flow']);
});
