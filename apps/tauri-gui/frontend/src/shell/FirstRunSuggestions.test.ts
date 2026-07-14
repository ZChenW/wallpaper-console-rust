import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { readFile, unlink, writeFile } from 'node:fs/promises';
import test from 'node:test';

import { Children, isValidElement, type ReactElement, type ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ts from 'typescript';

async function importTsxModule(): Promise<typeof import('./FirstRunSuggestions.tsx')> {
  const sourceUrl = new URL('./FirstRunSuggestions.tsx', import.meta.url);
  const outputUrl = new URL(`./.FirstRunSuggestions.test-${randomUUID()}.mjs`, import.meta.url);
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

test('renders nothing when detection has no suggestions', async () => {
  const { FirstRunSuggestions } = await importTsxModule();
  assert.equal(FirstRunSuggestions({
    suggestions: [],
    onAddDirectory: () => assert.fail('empty suggestions must not add a directory'),
    onScanWallpaperEngine: () => assert.fail('empty suggestions must not scan'),
  }), null);
});

test('renders concise accessible suggestions without triggering work on mount', async () => {
  const { FirstRunSuggestions } = await importTsxModule();
  const calls: string[] = [];
  const tree = FirstRunSuggestions({
    suggestions: [
      { kind: 'directory', label: 'Downloads', path: '/home/demo/Downloads' },
      {
        kind: 'wallpaperEngine',
        roots: [
          '/mnt/steam/steamapps/workshop/content/431960',
          '/home/demo/.steam/steamapps/workshop/content/431960',
        ],
      },
    ],
    onAddDirectory: (path) => calls.push(`directory:${path}`),
    onScanWallpaperEngine: () => calls.push('wallpaper-engine'),
  });
  const markup = renderToStaticMarkup(tree);

  assert.deepEqual(calls, [], 'rendering suggestions must never confirm them');
  assert.match(markup, /aria-labelledby="first-run-suggestions-title"/);
  assert.match(markup, /Suggested sources/);
  assert.match(markup, /Downloads/);
  assert.ok(markup.includes('/home/demo/Downloads'));
  assert.match(markup, /Add Downloads/);
  assert.match(markup, /Wallpaper Engine/);
  assert.match(markup, /Scan Wallpaper Engine/);
  assert.ok(markup.includes('/mnt/steam/steamapps/workshop/content/431960'));
  assert.ok(markup.includes('/home/demo/.steam/steamapps/workshop/content/431960'));
});

test('directory and Wallpaper Engine callbacks run only after their explicit buttons', async () => {
  const { FirstRunSuggestions } = await importTsxModule();
  const calls: string[] = [];
  const tree = FirstRunSuggestions({
    suggestions: [
      { kind: 'directory', label: 'Downloads', path: '/home/demo/Downloads' },
      { kind: 'wallpaperEngine' },
    ],
    onAddDirectory: (path) => calls.push(`directory:${path}`),
    onScanWallpaperEngine: () => calls.push('wallpaper-engine'),
  });
  const [addDirectory] = findElements(tree, (element) => (
    element.type === 'button' && element.props['data-first-run-action'] === 'add-directory'
  ));
  const [scanWallpaperEngine] = findElements(tree, (element) => (
    element.type === 'button' && element.props['data-first-run-action'] === 'scan-wallpaper-engine'
  ));

  assert.ok(addDirectory);
  assert.ok(scanWallpaperEngine);
  assert.deepEqual(calls, []);
  (addDirectory.props.onClick as () => void)();
  assert.deepEqual(calls, ['directory:/home/demo/Downloads']);
  (scanWallpaperEngine.props.onClick as () => void)();
  assert.deepEqual(calls, ['directory:/home/demo/Downloads', 'wallpaper-engine']);
});

test('Wallpaper Engine suggestion remains useful when detector has no roots', async () => {
  const { FirstRunSuggestions } = await importTsxModule();
  const markup = renderToStaticMarkup(FirstRunSuggestions({
    suggestions: [{ kind: 'wallpaperEngine', roots: [] }],
    onAddDirectory: () => undefined,
    onScanWallpaperEngine: () => undefined,
  }));

  assert.match(markup, /Wallpaper Engine content was detected/);
  assert.match(markup, /Nothing is scanned until you confirm/);
  assert.match(markup, /Scan Wallpaper Engine/);
});
