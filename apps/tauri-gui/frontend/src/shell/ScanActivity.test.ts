import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { readFile, unlink, writeFile } from 'node:fs/promises';
import test from 'node:test';

import { Children, isValidElement, type ReactElement, type ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ts from 'typescript';

async function importTsxModule(): Promise<typeof import('./ScanActivity.tsx')> {
  const sourceUrl = new URL('./ScanActivity.tsx', import.meta.url);
  const outputUrl = new URL(`./.ScanActivity.test-${randomUUID()}.mjs`, import.meta.url);
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
  return [
    ...matches,
    ...Children.toArray(node.props.children).flatMap((child) => findElements(child, predicate)),
  ];
}

test('hidden presentation renders nothing', async () => {
  const { ScanActivity } = await importTsxModule();
  assert.equal(ScanActivity({
    presentation: { kind: 'hidden' },
    onCancel: () => assert.fail('hidden scan must not cancel'),
    onDismiss: () => assert.fail('hidden scan must not dismiss'),
  }), null);
});

test('running scan is non-modal, exposes coarse elapsed time, and can cancel once', async () => {
  const { ScanActivity } = await importTsxModule();
  let cancelCount = 0;
  const tree = ScanActivity({
    presentation: { kind: 'running', nonModal: true, canCancel: true, elapsedMs: 12_987 },
    onCancel: () => { cancelCount += 1; },
    onDismiss: () => assert.fail('running scan must not dismiss'),
  });
  const markup = renderToStaticMarkup(tree);
  const [cancelButton] = findElements(tree, (element) => element.type === 'button');

  assert.match(markup, /role="status"/);
  assert.match(markup, /data-non-modal="true"/);
  assert.doesNotMatch(markup, /aria-modal/);
  assert.match(markup, /12s/);
  assert.doesNotMatch(markup, /12987/);
  assert.equal(cancelButton.props.disabled, false);
  (cancelButton.props.onClick as () => void)();
  assert.equal(cancelCount, 1);
});

test('running scan renders determinate progress when total hint is usable', async () => {
  const { ScanActivity } = await importTsxModule();
  const tree = ScanActivity({
    presentation: { kind: 'running', nonModal: true, canCancel: true, elapsedMs: 501 },
    progress: { scanned: 25, totalHint: 100 },
    onCancel: () => undefined,
    onDismiss: () => assert.fail('running scan must not dismiss'),
  });
  const [progressBar] = findElements(tree, (element) => element.type === 'progress');

  assert.ok(progressBar);
  assert.equal(progressBar.props['aria-label'], 'Wallpaper scan progress');
  assert.equal(progressBar.props.value, 25);
  assert.equal(progressBar.props.max, 100);
});

test('running scan renders indeterminate progress without a usable total hint', async () => {
  const { ScanActivity } = await importTsxModule();
  const tree = ScanActivity({
    presentation: { kind: 'running', nonModal: true, canCancel: true, elapsedMs: 501 },
    progress: { scanned: 25, totalHint: 0 },
    onCancel: () => undefined,
    onDismiss: () => assert.fail('running scan must not dismiss'),
  });
  const [progressBar] = findElements(tree, (element) => element.type === 'progress');

  assert.ok(progressBar);
  assert.equal(progressBar.props['aria-label'], 'Wallpaper scan progress');
  assert.equal(progressBar.props.value, undefined);
  assert.equal(progressBar.props.max, undefined);
});

test('cancelling scan disables repeated cancellation', async () => {
  const { ScanActivity } = await importTsxModule();
  const tree = ScanActivity({
    presentation: { kind: 'cancelling', nonModal: true, canCancel: false, elapsedMs: 1_500 },
    onCancel: () => assert.fail('cancelling scan must not cancel twice'),
    onDismiss: () => assert.fail('cancelling scan must not dismiss'),
  });
  const markup = renderToStaticMarkup(tree);
  const [cancelButton] = findElements(tree, (element) => element.type === 'button');

  assert.match(markup, /Cancelling/);
  assert.equal(cancelButton.props.disabled, true);
  assert.equal(cancelButton.props.onClick, undefined);
});

test('cancelled scan remains non-modal and can be dismissed', async () => {
  const { ScanActivity } = await importTsxModule();
  let dismissCount = 0;
  const tree = ScanActivity({
    presentation: { kind: 'cancelled', nonModal: true },
    onCancel: () => assert.fail('cancelled scan must not cancel'),
    onDismiss: () => { dismissCount += 1; },
  });
  const markup = renderToStaticMarkup(tree);
  const [dismissButton] = findElements(tree, (element) => element.type === 'button');

  assert.match(markup, /Scan cancelled/);
  assert.match(markup, /Completed source updates were kept/);
  assert.match(markup, /data-non-modal="true"/);
  assert.doesNotMatch(markup, /aria-modal/);
  (dismissButton.props.onClick as () => void)();
  assert.equal(dismissCount, 1);
});

test('elapsed display changes at seconds, not milliseconds', async () => {
  const { formatScanElapsed } = await importTsxModule();
  assert.equal(formatScanElapsed(500), '< 1s');
  assert.equal(formatScanElapsed(1_001), '1s');
  assert.equal(formatScanElapsed(1_999), '1s');
  assert.equal(formatScanElapsed(61_999), '1m 1s');
  assert.equal(formatScanElapsed(Number.NaN), '< 1s');
});
