import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { readFile, unlink, writeFile } from 'node:fs/promises';
import test from 'node:test';

import { Children, isValidElement, type ReactElement, type ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ts from 'typescript';

async function importTsxModule(): Promise<typeof import('./LibraryRepairPrompt.tsx')> {
  const sourceUrl = new URL('./LibraryRepairPrompt.tsx', import.meta.url);
  const outputUrl = new URL(`./.LibraryRepairPrompt.test-${randomUUID()}.mjs`, import.meta.url);
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
    const Component = node.type as (props: Record<string, unknown>) => ReactNode;
    const rendered = Component(node.props);
    return [...matches, ...findElements(rendered, predicate)];
  }
  return [
    ...matches,
    ...Children.toArray(node.props.children as ReactNode).flatMap((child) => findElements(child, predicate)),
  ];
}

test('repair prompt appears only for a detected fault and exposes details on demand', async () => {
  const { default: LibraryRepairPrompt } = await importTsxModule();
  assert.equal(LibraryRepairPrompt({ fault: null, pending: false, onRepair: () => undefined }), null);

  const calls: string[] = [];
  const tree = LibraryRepairPrompt({
    fault: {
      message: 'Library database needs repair',
      technicalDetails: 'page 12 is malformed',
    },
    pending: false,
    onRepair: () => calls.push('repair'),
  });
  const markup = renderToStaticMarkup(tree);
  const [button] = findElements(tree, (element) => element.props['data-library-repair'] === true);

  assert.match(markup, /Library database needs repair/);
  assert.match(markup, /Repair library/);
  assert.match(markup, /<details/);
  assert.match(markup, /page 12 is malformed/);
  assert.equal(button.props.disabled, false);
  (button.props.onClick as () => void)();
  assert.deepEqual(calls, ['repair']);

  const busy = LibraryRepairPrompt({
    fault: { message: 'Library database needs repair', technicalDetails: 'broken' },
    pending: true,
    onRepair: () => undefined,
  });
  const [busyButton] = findElements(
    busy,
    (element) => element.props['data-library-repair'] === true,
  );
  assert.equal(busyButton.props.disabled, true);
  assert.match(renderToStaticMarkup(busy), /Repairing/);
});
