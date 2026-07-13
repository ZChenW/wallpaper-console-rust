import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { readFile, unlink, writeFile } from 'node:fs/promises';
import test from 'node:test';

import { Children, isValidElement, type ReactElement, type ReactNode } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import ts from 'typescript';

import type { CommandResult, SourceDTO } from '../api/types.ts';

async function importTsxModule(): Promise<typeof import('./SourcePanel.tsx')> {
  const sourceUrl = new URL('./SourcePanel.tsx', import.meta.url);
  const outputUrl = new URL(`./.SourcePanel.test-${randomUUID()}.mjs`, import.meta.url);
  const source = await readFile(sourceUrl, 'utf8');
  const output = ts.transpileModule(source, {
    compilerOptions: {
      jsx: ts.JsxEmit.ReactJSX,
      module: ts.ModuleKind.ESNext,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourceUrl.pathname,
  }).outputText.replace("from './useWallpaperSources';", "from './useWallpaperSources.ts';");

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

const sources: SourceDTO[] = [
  {
    id: 41,
    path: '/home/demo/Pictures/wallpapers',
    displayName: 'Downloaded',
    kind: 'directory',
    recursive: true,
    availability: 'available',
    addedAt: '2026-07-14T00:00:00Z',
    exists: true,
    isWE: false,
    label: 'Downloaded',
  },
  {
    id: 77,
    path: '/mnt/steam/steamapps/workshop/content/431960',
    displayName: 'Wallpaper Engine',
    kind: 'wallpaper_engine_workshop',
    recursive: false,
    availability: 'offline',
    addedAt: '2026-07-14T00:00:00Z',
    exists: false,
    isWE: true,
    label: 'Wallpaper Engine',
  },
];

function noopViewProps() {
  return {
    open: true,
    sources,
    loading: false,
    loadError: null,
    pendingOperation: null,
    removeCandidateId: null,
    onClose: () => undefined,
    onReload: () => undefined,
    onAdd: () => undefined,
    onScanWallpaperEngine: () => undefined,
    onRename: (_id: number, _displayName: string) => undefined,
    onSetRecursive: (_id: number, _recursive: boolean) => undefined,
    onRefresh: (_id: number) => undefined,
    onRequestRemove: (_id: number) => undefined,
    onCancelRemove: () => undefined,
    onRemove: (_id: number) => undefined,
  };
}

test('closed source panel renders nothing', async () => {
  const { SourcePanelView } = await importTsxModule();
  assert.equal(SourcePanelView({ ...noopViewProps(), open: false }), null);
});

test('renders a compact accessible drawer with source kind and honest availability', async () => {
  const { SourcePanelView } = await importTsxModule();
  const markup = renderToStaticMarkup(SourcePanelView(noopViewProps()));

  assert.match(markup, /role="dialog"/);
  assert.match(markup, /aria-label="Wallpaper sources"/);
  assert.match(markup, /Downloaded/);
  assert.match(markup, /\/home\/demo\/Pictures\/wallpapers/);
  assert.match(markup, /Directory/);
  assert.match(markup, /Available/);
  assert.match(markup, /Wallpaper Engine/);
  assert.match(markup, /Offline/);
  assert.match(markup, /indexed wallpapers are kept/);
  assert.match(markup, /data-source-id="41"/);
  assert.match(markup, /data-source-id="77"/);
  assert.match(markup, /aria-label="Close wallpaper sources"/);
});

test('renders loading, load failure with retry, and empty-library states', async () => {
  const { SourcePanelView } = await importTsxModule();

  const loading = renderToStaticMarkup(SourcePanelView({
    ...noopViewProps(),
    sources: [],
    loading: true,
  }));
  assert.match(loading, /Loading sources/);
  assert.match(loading, /role="status"/);

  const failedTree = SourcePanelView({
    ...noopViewProps(),
    sources: [],
    loadError: 'database is locked',
  });
  const failed = renderToStaticMarkup(failedTree);
  assert.match(failed, /database is locked/);
  assert.match(failed, /role="alert"/);
  assert.match(failed, /Retry/);

  const empty = renderToStaticMarkup(SourcePanelView({ ...noopViewProps(), sources: [] }));
  assert.match(empty, /No wallpaper sources yet/);
  assert.match(empty, /Add a folder or scan Wallpaper Engine when you are ready/);
});

test('source row actions use stable IDs and reject an empty inline rename', async () => {
  const { SourcePanelView } = await importTsxModule();
  const calls: unknown[][] = [];
  const tree = SourcePanelView({
    ...noopViewProps(),
    onRename: (id, name) => calls.push(['rename', id, name]),
    onSetRecursive: (id, recursive) => calls.push(['recursive', id, recursive]),
    onRefresh: (id) => calls.push(['refresh', id]),
    onRequestRemove: (id) => calls.push(['request-remove', id]),
  });
  const [renameForm] = findElements(tree, (element) => element.props['data-source-action'] === 'rename:41');
  const [recursive] = findElements(tree, (element) => element.props['data-source-action'] === 'recursive:41');
  const [refresh] = findElements(tree, (element) => element.props['data-source-action'] === 'refresh:77');
  const [requestRemove] = findElements(tree, (element) => element.props['data-source-action'] === 'request-remove:77');

  assert.ok(renameForm);
  assert.ok(recursive);
  assert.ok(refresh);
  assert.ok(requestRemove);

  let prevented = 0;
  const submitRename = (value: string) => (renameForm.props.onSubmit as (event: unknown) => void)({
    preventDefault: () => { prevented += 1; },
    currentTarget: {
      elements: { namedItem: (name: string) => name === 'displayName' ? { value } : null },
    },
  });
  submitRename('   ');
  submitRename('  Personal picks  ');
  (recursive.props.onChange as (event: unknown) => void)({ currentTarget: { checked: false } });
  (refresh.props.onClick as () => void)();
  (requestRemove.props.onClick as () => void)();

  assert.equal(prevented, 2);
  assert.deepEqual(calls, [
    ['rename', 41, 'Personal picks'],
    ['recursive', 41, false],
    ['refresh', 77],
    ['request-remove', 77],
  ]);
});

test('removal requires an explicit confirmation that promises files are untouched', async () => {
  const { SourcePanelView } = await importTsxModule();
  const calls: unknown[][] = [];
  const tree = SourcePanelView({
    ...noopViewProps(),
    removeCandidateId: 41,
    onCancelRemove: () => calls.push(['cancel']),
    onRemove: (id) => calls.push(['remove', id]),
  });
  const markup = renderToStaticMarkup(tree);
  const [cancel] = findElements(tree, (element) => element.props['data-source-action'] === 'cancel-remove:41');
  const [confirm] = findElements(tree, (element) => element.props['data-source-action'] === 'confirm-remove:41');

  assert.match(markup, /role="alertdialog"/);
  assert.match(markup, /only removes this source from the library index/i);
  assert.match(markup, /does not delete wallpaper files/i);
  (cancel.props.onClick as () => void)();
  (confirm.props.onClick as () => void)();
  assert.deepEqual(calls, [['cancel'], ['remove', 41]]);
});

test('a pending source operation disables every mutating control without trapping close', async () => {
  const { SourcePanelView } = await importTsxModule();
  const tree = SourcePanelView({
    ...noopViewProps(),
    pendingOperation: 'refresh:41',
    removeCandidateId: 41,
  });
  const markup = renderToStaticMarkup(tree);
  const mutatingControls = findElements(tree, (element) => element.props['data-source-mutating'] === true);
  const [close] = findElements(tree, (element) => element.props['aria-label'] === 'Close wallpaper sources');

  assert.match(markup, /Refreshing source/);
  assert.match(markup, /aria-busy="true"/);
  assert.ok(mutatingControls.length > 6);
  assert.equal(mutatingControls.every((element) => element.props.disabled === true), true);
  assert.equal(close.props.disabled, undefined);
});

const success: CommandResult = { success: true, stdout: 'ok', stderr: '', exitCode: 0 };
const failure: CommandResult = {
  success: false,
  stdout: 'source saved',
  stderr: 'permission denied',
  exitCode: 1,
  error: {
    kind: 'io',
    message: 'Could not scan folder',
    detail: '/private/wallpapers',
    recoverable: true,
    suggestion: 'Check folder permissions',
  },
};

test('folder picker cancellation is quiet while command failures produce useful settings notices', async () => {
  const { runAddSourceAction } = await importTsxModule();
  const notices: unknown[] = [];
  const onNotice = (notice: unknown) => notices.push(notice);

  await runAddSourceAction(async () => ({ kind: 'cancelled' }), onNotice);
  assert.deepEqual(notices, []);

  await runAddSourceAction(
    async () => ({ kind: 'completed', path: '/private/wallpapers', result: failure }),
    onNotice,
  );
  assert.equal(notices.length, 1);
  assert.deepEqual(notices[0], {
    channel: 'settings',
    severity: 'error',
    message: 'Could not finish adding folder',
    technicalDetails: [
      'Could not scan folder',
      'Check folder permissions',
      '/private/wallpapers',
      'permission denied',
      'source saved',
      'Exit code: 1',
    ].join('\n'),
  });
});

test('Wallpaper Engine scan brackets the command and reports success or thrown failure on scan channel', async () => {
  const { runWallpaperEngineScanAction } = await importTsxModule();
  const events: string[] = [];
  const notices: Array<{ severity: string; message: string; technicalDetails?: string }> = [];

  await runWallpaperEngineScanAction(
    async () => success,
    (notice) => {
      notices.push(notice);
      events.push(`notice:${notice.severity}`);
    },
    () => events.push('started'),
    () => events.push('finished'),
  );
  assert.deepEqual(events, ['started', 'notice:info', 'notice:success', 'finished']);
  assert.deepEqual(notices.map(({ severity, message }) => ({ severity, message })), [
    { severity: 'info', message: 'Scanning Wallpaper Engine' },
    { severity: 'success', message: 'Wallpaper Engine scan finished' },
  ]);

  events.length = 0;
  notices.length = 0;
  await runWallpaperEngineScanAction(
    async () => { throw new Error('Steam library is unavailable'); },
    (notice) => {
      notices.push(notice);
      events.push(`notice:${notice.severity}`);
    },
    () => events.push('started'),
    () => events.push('finished'),
  );
  assert.deepEqual(events, ['started', 'notice:info', 'notice:error', 'finished']);
  assert.equal(notices[1]?.message, 'Could not scan Wallpaper Engine');
  assert.equal(notices[1]?.technicalDetails, 'Steam library is unavailable');
});

test('single-source refresh also brackets backend scan polling', async () => {
  const { runSourceRefreshAction } = await importTsxModule();
  const events: string[] = [];

  const refreshed = await runSourceRefreshAction(
    async () => success,
    (notice) => events.push(`notice:${notice.severity}:${notice.message}`),
    () => events.push('started'),
    () => events.push('finished'),
  );

  assert.equal(refreshed, true);
  assert.deepEqual(events, [
    'started',
    'notice:info:Refreshing source',
    'notice:success:Source refresh finished',
    'finished',
  ]);
});
