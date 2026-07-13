import assert from 'node:assert/strict';
import test from 'node:test';

import {
  createLibraryBrowserApi,
  createSourceMutationApi,
  type InvokeFn,
  type LibraryBrowserQueryDTO,
} from './bridge.ts';

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
    offset: 20,
    limit: 40,
  };

  await api.libraryBrowserPage(query);
  await api.libraryBrowserRandom(query);

  assert.deepEqual(calls, [
    { command: 'library_browser_page', args: { query } },
    { command: 'library_browser_random', args: { query } },
  ]);
});
