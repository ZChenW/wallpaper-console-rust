import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { readFile, unlink, writeFile } from 'node:fs/promises';
import test from 'node:test';

import { Children, isValidElement, type ReactElement, type ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ts from 'typescript';

import {
  EMPTY_FEEDBACK_STATE,
  feedbackReducer,
  type FeedbackAction,
  type FeedbackState,
} from './feedbackState.ts';

async function importTsxModule(): Promise<typeof import('./FeedbackOverlay.tsx')> {
  const sourceUrl = new URL('./FeedbackOverlay.tsx', import.meta.url);
  const outputUrl = new URL(`./.FeedbackOverlay.test-${randomUUID()}.mjs`, import.meta.url);
  const source = await readFile(sourceUrl, 'utf8');
  const output = ts.transpileModule(source, {
    compilerOptions: {
      jsx: ts.JsxEmit.ReactJSX,
      module: ts.ModuleKind.ESNext,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourceUrl.pathname,
  }).outputText.replace("from './feedbackState';", "from './feedbackState.ts';");

  await writeFile(outputUrl, output, 'utf8');
  try {
    return await import(outputUrl.href);
  } finally {
    await unlink(outputUrl);
  }
}

function show(
  state: Readonly<FeedbackState>,
  channel: 'apply' | 'scan' | 'settings' | 'system',
  severity: 'success' | 'info' | 'warning' | 'error',
  message: string,
): FeedbackState {
  return feedbackReducer(state, { type: 'show', channel, severity, message, nowMs: 1_000 });
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

test('renders concurrent channels with severity, accessible labels, and escaped details', async () => {
  const { FeedbackOverlay } = await importTsxModule();
  let state = show(EMPTY_FEEDBACK_STATE, 'apply', 'success', 'Wallpaper applied');
  state = show(state, 'scan', 'info', 'Scanning library');
  state = show(state, 'settings', 'warning', 'Some settings were skipped');
  state = show(state, 'system', 'error', 'Backend unavailable');

  const markup = renderToStaticMarkup(FeedbackOverlay({
    state,
    nowMs: 2_500,
    dispatch: () => undefined,
    technicalDetails: {
      system: '<img src=x onerror=alert(1)> backend exited 1',
    },
  }));

  for (const message of [
    'Wallpaper applied',
    'Scanning library',
    'Some settings were skipped',
    'Backend unavailable',
  ]) {
    assert.match(markup, new RegExp(message));
  }
  for (const severity of ['success', 'info', 'warning', 'error']) {
    assert.match(markup, new RegExp(`data-feedback-severity="${severity}"`));
  }
  assert.match(markup, /aria-label="Notifications"/);
  assert.match(markup, /<details/);
  assert.match(markup, /Technical details/);
  assert.doesNotMatch(markup, /<img src=x/);
  assert.match(markup, /&lt;img src=x onerror=alert\(1\)&gt;/);
});

test('dispatches dismiss and hover pause/resume with the observed timestamp', async () => {
  const { FeedbackOverlay } = await importTsxModule();
  const state = show(EMPTY_FEEDBACK_STATE, 'apply', 'success', 'Wallpaper applied');
  const actions: FeedbackAction[] = [];
  const tree = FeedbackOverlay({
    state,
    nowMs: 2_000,
    dispatch: (action) => actions.push(action),
  });
  const [card] = findElements(tree, (element) => element.props['data-feedback-card'] === 'apply');
  const [closeButton] = findElements(tree, (element) => (
    element.type === 'button' && element.props['aria-label'] === 'Dismiss apply notification'
  ));

  assert.ok(card);
  assert.ok(closeButton);
  (card.props.onMouseEnter as () => void)();
  (card.props.onMouseLeave as () => void)();
  (closeButton.props.onClick as () => void)();

  assert.deepEqual(actions, [
    { type: 'pause', channel: 'apply', nowMs: 2_000 },
    { type: 'resume', channel: 'apply', nowMs: 2_000 },
    { type: 'dismiss', channel: 'apply' },
  ]);
});

test('puts countdown progress last on timed cards and omits it for persistent errors', async () => {
  const { FeedbackOverlay } = await importTsxModule();
  let state = show(EMPTY_FEEDBACK_STATE, 'apply', 'success', 'Wallpaper applied');
  state = show(state, 'system', 'error', 'Backend unavailable');
  const tree = FeedbackOverlay({ state, nowMs: 2_500, dispatch: () => undefined });
  const [timedCard] = findElements(tree, (element) => element.props['data-feedback-card'] === 'apply');
  const [errorCard] = findElements(tree, (element) => element.props['data-feedback-card'] === 'system');
  const timedChildren = Children.toArray(timedCard.props.children);
  const errorChildren = Children.toArray(errorCard.props.children);
  const lastTimedChild = timedChildren.at(-1);

  assert.ok(isValidElement<Record<string, unknown>>(lastTimedChild));
  assert.equal(lastTimedChild.props['data-feedback-progress'], 'apply');
  assert.equal(lastTimedChild.props.role, 'progressbar');
  assert.equal(lastTimedChild.props['aria-valuenow'], 50);
  assert.equal(
    errorChildren.some((child) => (
      isValidElement<Record<string, unknown>>(child) && 'data-feedback-progress' in child.props
    )),
    false,
  );
});
