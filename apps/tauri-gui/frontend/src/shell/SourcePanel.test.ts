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
  {
    id: 99,
    path: '/media/archive/wallpapers',
    displayName: 'Archive',
    kind: 'directory',
    recursive: false,
    availability: 'unknown',
    addedAt: '2026-07-14T00:00:00Z',
    exists: false,
    isWE: false,
    label: 'Archive',
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
    editingSourceId: null,
    renameDraft: '',
    onClose: () => undefined,
    onReload: () => undefined,
    onAdd: () => undefined,
    onRefreshAll: () => undefined,
    onScanWallpaperEngine: () => undefined,
    onRename: (_id: number, _displayName: string) => undefined,
    onStartRename: (_source: SourceDTO) => undefined,
    onChangeRenameDraft: (_displayName: string) => undefined,
    onCancelRename: () => undefined,
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

test('source panel reloads only when an already-opened drawer is reopened', async () => {
  const { transitionSourcePanelVisibility } = await importTsxModule();
  let visibility = { open: false, hasOpened: false };

  let transition = transitionSourcePanelVisibility(visibility, true);
  assert.equal(transition.reload, false, 'the hook mount already loads the first opening');
  visibility = transition.next;

  transition = transitionSourcePanelVisibility(visibility, false);
  assert.equal(transition.reload, false);
  visibility = transition.next;

  transition = transitionSourcePanelVisibility(visibility, true);
  assert.equal(transition.reload, true, 'reopening reconciles mutations made while hidden');
  visibility = transition.next;

  transition = transitionSourcePanelVisibility(visibility, true);
  assert.equal(transition.reload, false, 'ordinary rerenders while open do not reload');
});

test('source panel exit accepts one request and respects reduced motion', async () => {
  const { beginSourcePanelExit } = await importTsxModule();

  assert.deepEqual(beginSourcePanelExit('open', false), {
    accepted: true,
    next: 'exiting',
    delayMs: 160,
  });
  assert.deepEqual(beginSourcePanelExit('exiting', false), {
    accepted: false,
    next: 'exiting',
    delayMs: 0,
  });
  assert.deepEqual(beginSourcePanelExit('open', true), {
    accepted: true,
    next: 'exiting',
    delayMs: 0,
  });
});

test('source drawer backdrop closes only when the backdrop itself is pressed', async () => {
  const { SourcePanelView } = await importTsxModule();
  const calls: string[] = [];
  const tree = SourcePanelView({
    ...noopViewProps(),
    onClose: () => calls.push('close'),
  });
  const [backdrop] = findElements(
    tree,
    (element) => element.props.className === 'source-panel__backdrop',
  );
  const onMouseDown = backdrop.props.onMouseDown as (event: unknown) => void;
  const backdropTarget = {};

  onMouseDown({ currentTarget: backdropTarget, target: {} });
  assert.deepEqual(calls, [], 'presses inside the drawer must not close it');

  onMouseDown({ currentTarget: backdropTarget, target: backdropTarget });
  assert.deepEqual(calls, ['close']);
});

test('source drawer autofocuses close and Escape closes or dismisses removal first', async () => {
  const { SourcePanelView } = await importTsxModule();
  const calls: string[] = [];
  const tree = SourcePanelView({
    ...noopViewProps(),
    onClose: () => calls.push('close'),
  });
  const [dialog] = findElements(tree, (element) => element.props.role === 'dialog');
  const [close] = findElements(tree, (element) => element.props['aria-label'] === 'Close wallpaper sources');
  assert.equal(close.props.autoFocus, true);
  (dialog.props.onKeyDown as (event: unknown) => void)({
    key: 'Escape',
    preventDefault: () => undefined,
  });
  assert.deepEqual(calls, ['close']);

  const confirmTree = SourcePanelView({
    ...noopViewProps(),
    removeCandidateId: 41,
    onClose: () => calls.push('unexpected-close'),
    onCancelRemove: () => calls.push('cancel-remove'),
  });
  const [confirmDialog] = findElements(confirmTree, (element) => element.props.role === 'dialog');
  (confirmDialog.props.onKeyDown as (event: unknown) => void)({
    key: 'Escape',
    preventDefault: () => undefined,
  });
  assert.deepEqual(calls, ['close', 'cancel-remove']);
});

test('source drawer exposes Back to settings only when a parent destination is provided', async () => {
  const { SourcePanelView } = await importTsxModule();
  const calls: string[] = [];
  const nestedTree = SourcePanelView({
    ...noopViewProps(),
    onBack: () => calls.push('back'),
  });
  const [back] = findElements(
    nestedTree,
    (element) => element.props['aria-label'] === 'Back to settings',
  );

  assert.ok(back);
  (back.props.onClick as () => void)();
  assert.deepEqual(calls, ['back']);

  const rootTree = SourcePanelView(noopViewProps());
  assert.equal(findElements(
    rootTree,
    (element) => element.props['aria-label'] === 'Back to settings',
  ).length, 0);
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
  assert.match(markup, /Availability unknown/);
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

test('source rows expose always-visible labelled icon actions with stable IDs', async () => {
  const { SourcePanelView } = await importTsxModule();
  const calls: unknown[][] = [];
  const tree = SourcePanelView({
    ...noopViewProps(),
    onStartRename: (source) => calls.push(['start-rename', source.id]),
    onSetRecursive: (id, recursive) => calls.push(['recursive', id, recursive]),
    onRefresh: (id) => calls.push(['refresh', id]),
    onRequestRemove: (id) => calls.push(['request-remove', id]),
  });
  const [rename] = findElements(tree, (element) => element.props['data-source-action'] === 'start-rename:41');
  const [recursive] = findElements(tree, (element) => element.props['data-source-action'] === 'recursive:41');
  const [refresh] = findElements(tree, (element) => element.props['data-source-action'] === 'refresh:77');
  const [requestRemove] = findElements(tree, (element) => element.props['data-source-action'] === 'request-remove:77');

  assert.equal(rename.props['aria-label'], 'Rename Downloaded');
  assert.equal(rename.props.title, 'Rename Downloaded');
  assert.equal(refresh.props['aria-label'], 'Refresh Wallpaper Engine');
  assert.equal(refresh.props.title, 'Refresh Wallpaper Engine');
  assert.equal(requestRemove.props['aria-label'], 'Remove Wallpaper Engine');
  assert.equal(requestRemove.props.title, 'Remove Wallpaper Engine');
  assert.ok(recursive);

  (rename.props.onClick as () => void)();
  (recursive.props.onChange as (event: unknown) => void)({ currentTarget: { checked: false } });
  (refresh.props.onClick as () => void)();
  (requestRemove.props.onClick as () => void)();

  assert.deepEqual(calls, [
    ['start-rename', 41],
    ['recursive', 41, false],
    ['refresh', 77],
    ['request-remove', 77],
  ]);
});

test('inline alias editor is controlled, saves on Enter, and cancels on Escape', async () => {
  const { SourcePanelView } = await importTsxModule();
  const calls: unknown[][] = [];
  const tree = SourcePanelView({
    ...noopViewProps(),
    editingSourceId: 41,
    renameDraft: 'Personal picks',
    onRename: (id, name) => calls.push(['rename', id, name]),
    onChangeRenameDraft: (name) => calls.push(['change', name]),
    onCancelRename: () => calls.push(['cancel']),
  });
  const [form] = findElements(tree, (element) => element.props['data-source-action'] === 'rename:41');
  const [input] = findElements(tree, (element) => element.props['aria-label'] === 'Alias for Downloaded');

  assert.equal(input.props.value, 'Personal picks');
  assert.equal(input.props.autoFocus, true);
  (input.props.onChange as (event: unknown) => void)({ currentTarget: { value: 'New alias' } });

  let prevented = 0;
  (form.props.onSubmit as (event: unknown) => void)({ preventDefault: () => { prevented += 1; } });
  assert.equal(prevented, 1);
  assert.deepEqual(calls, [['change', 'New alias'], ['rename', 41, 'Personal picks']]);

  let stopped = 0;
  (input.props.onKeyDown as (event: unknown) => void)({
    key: 'Escape',
    preventDefault: () => { prevented += 1; },
    stopPropagation: () => { stopped += 1; },
  });
  assert.equal(stopped, 1, 'Escape must not bubble and close the drawer');
  assert.deepEqual(calls.at(-1), ['cancel']);
});

test('inline alias editor cancels without renaming when focus moves outside', async () => {
  const { SourcePanelView } = await importTsxModule();
  const calls: unknown[][] = [];
  const tree = SourcePanelView({
    ...noopViewProps(),
    editingSourceId: 41,
    renameDraft: 'Unsaved alias',
    onRename: (id, name) => calls.push(['rename', id, name]),
    onCancelRename: () => calls.push(['cancel']),
  });
  const [input] = findElements(
    tree,
    (element) => element.props['aria-label'] === 'Alias for Downloaded',
  );

  (input.props.onBlur as () => void)();

  assert.deepEqual(calls, [['cancel']]);
});

test('rename editor closes only after its own successful request', async () => {
  const { renameEditorAfterResult } = await importTsxModule();
  const editor = { sourceId: 41, draft: 'Personal picks' };

  assert.deepEqual(renameEditorAfterResult(editor, 41, false), editor);
  assert.equal(renameEditorAfterResult(editor, 41, true), null);
  assert.deepEqual(
    renameEditorAfterResult({ sourceId: 77, draft: 'Newer edit' }, 41, true),
    { sourceId: 77, draft: 'Newer edit' },
  );
});

test('availability badges use green, red, and yellow status backgrounds', async () => {
  const { SourcePanelView } = await importTsxModule();
  const tree = SourcePanelView(noopViewProps());
  const badge = (availability: string) => findElements(
    tree,
    (element) => element.props['data-source-availability'] === availability,
  )[0];

  assert.equal(badge('available').props.style.background, 'rgb(46 155 89 / 16%)');
  assert.equal(badge('offline').props.style.background, 'rgb(217 75 75 / 16%)');
  assert.equal(badge('unknown').props.style.background, 'rgb(183 135 0 / 16%)');
});

test('refresh all is available only when configured sources can be scanned', async () => {
  const { SourcePanelView } = await importTsxModule();
  const calls: string[] = [];
  const tree = SourcePanelView({
    ...noopViewProps(),
    onRefreshAll: () => calls.push('refresh-all'),
  });
  const [refreshAll] = findElements(
    tree,
    (element) => element.props['data-source-action'] === 'refresh-all',
  );

  assert.ok(refreshAll);
  assert.equal(refreshAll.props.disabled, false);
  (refreshAll.props.onClick as () => void)();
  assert.deepEqual(calls, ['refresh-all']);

  const empty = SourcePanelView({ ...noopViewProps(), sources: [] });
  const [emptyRefreshAll] = findElements(
    empty,
    (element) => element.props['data-source-action'] === 'refresh-all',
  );
  assert.equal(emptyRefreshAll.props.disabled, true);

  const pending = renderToStaticMarkup(SourcePanelView({
    ...noopViewProps(),
    pendingOperation: 'refreshAll',
  }));
  assert.match(pending, /Refreshing all sources/);
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

test('all-source refresh uses distinct feedback and always closes scan polling', async () => {
  const { runAllSourcesRefreshAction } = await importTsxModule();
  const events: string[] = [];

  const refreshed = await runAllSourcesRefreshAction(
    async () => success,
    (notice) => events.push(`notice:${notice.severity}:${notice.message}`),
    () => events.push('started'),
    () => events.push('finished'),
  );

  assert.equal(refreshed, true);
  assert.deepEqual(events, [
    'started',
    'notice:info:Refreshing all sources',
    'notice:success:All sources refreshed',
    'finished',
  ]);
});
