import assert from 'node:assert/strict';
import test from 'node:test';

import {
  createFirstRunSuggestionApi,
  createLibraryBrowserApi,
  createRendererStatusApi,
  createRuntimeObservationApi,
  createSourceMutationApi,
  type InvokeFn,
  type LibraryBrowserQueryDTO,
} from './bridge.ts';

test('renderer status bridge invokes the unified read-only probe', async () => {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const statuses = {
    awww: { available: true, message: 'awww is installed.' },
    mpvpaper: { available: false, message: 'mpvpaper is unavailable.' },
    linuxWallpaperEngine: {
      available: true,
      message: 'linux-wallpaperengine is installed.',
    },
  };
  const invoke: InvokeFn = async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    return statuses as T;
  };

  const api = createRendererStatusApi(invoke);
  assert.deepEqual(await api.rendererStatuses(), statuses);
  assert.deepEqual(calls, [{ command: 'renderer_statuses', args: undefined }]);
});

test('first-run suggestion bridge invokes the read-only detector', async () => {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const invoke: InvokeFn = async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    return [{ kind: 'directory', label: 'Downloads', path: '/home/demo/Downloads' }] as T;
  };

  const api = createFirstRunSuggestionApi(invoke);
  assert.deepEqual(await api.firstRunSourceSuggestions(), [
    { kind: 'directory', label: 'Downloads', path: '/home/demo/Downloads' },
  ]);
  assert.deepEqual(calls, [{ command: 'first_run_source_suggestions', args: undefined }]);
});

test('runtime observation bridge invokes the read-only reconciliation command', async () => {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const invoke: InvokeFn = async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    return [] as T;
  };

  const api = createRuntimeObservationApi(invoke);
  assert.deepEqual(await api.runtimeWallpaperObservations(), []);
  assert.deepEqual(calls, [{ command: 'runtime_wallpaper_observations', args: undefined }]);
});

test('source mutation invokes use camelCase command arguments', async () => {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const invoke: InvokeFn = async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    return { success: true, stdout: '', stderr: '', exitCode: 0 } as T;
  };
  const api = createSourceMutationApi(invoke);

  await api.sourceRename(42, 'Curated');
  await api.sourceSetRecursive(42, false);
  await api.sourceRefresh(42);
  await api.sourceRemoveById(42);

  assert.deepEqual(calls, [
    { command: 'source_rename', args: { id: 42, displayName: 'Curated' } },
    { command: 'source_set_recursive', args: { id: 42, recursive: false } },
    { command: 'source_refresh', args: { id: 42 } },
    { command: 'source_remove_by_id', args: { id: 42 } },
  ]);
});

test('library browser invokes use one camelCase query object', async () => {
  const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
  const invoke: InvokeFn = async <T>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    return (command === 'library_browser_page' ? { total: 0, items: [] } : null) as T;
  };
  const api = createLibraryBrowserApi(invoke);
  const query: LibraryBrowserQueryDTO = {
    sourceId: 7,
    typeFilter: 'weScene',
    favoritesOnly: true,
    search: 'aurora ada',
    sort: 'nameDesc',
    cursor: 'page-20',
    limit: 40,
  };

  await api.libraryBrowserPage(query);
  await api.libraryBrowserRandom(query);
  await api.libraryWallpaperExists(42);

  assert.deepEqual(calls, [
    { command: 'library_browser_page', args: { query } },
    { command: 'library_browser_random', args: { query } },
    { command: 'library_wallpaper_exists', args: { wallpaperId: 42 } },
  ]);
});
