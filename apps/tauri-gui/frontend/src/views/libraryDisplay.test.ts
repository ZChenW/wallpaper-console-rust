import assert from 'node:assert/strict';
import test from 'node:test';

import { resolveLibraryDisplay } from './libraryDisplay.ts';

const grid = (overrides: Partial<Parameters<typeof resolveLibraryDisplay>[0]> = {}) =>
  resolveLibraryDisplay({
    initialLoading: false,
    hasLoadedOnce: true,
    total: 50,
    entryCount: 50,
    scanRunning: false,
    ...overrides,
  });

test('resolveLibraryDisplay shows grid when entries exist regardless of refresh state', () => {
  assert.equal(grid({ entryCount: 30, total: 50 }), 'grid');
  assert.equal(grid({ entryCount: 1 }), 'grid');
});

test('resolveLibraryDisplay shows loading before first page when no entries and no scan', () => {
  assert.equal(
    resolveLibraryDisplay({
      initialLoading: true,
      hasLoadedOnce: false,
      total: 0,
      entryCount: 0,
      scanRunning: false,
    }),
    'loading',
  );
});

test('resolveLibraryDisplay shows indexing when scan running and no entries', () => {
  assert.equal(
    resolveLibraryDisplay({
      initialLoading: true,
      hasLoadedOnce: false,
      total: 0,
      entryCount: 0,
      scanRunning: true,
    }),
    'indexing',
  );
  assert.equal(
    resolveLibraryDisplay({
      initialLoading: false,
      hasLoadedOnce: true,
      total: 0,
      entryCount: 0,
      scanRunning: true,
    }),
    'indexing',
  );
});

test('resolveLibraryDisplay shows empty only after first page with zero total and no scan', () => {
  assert.equal(
    resolveLibraryDisplay({
      initialLoading: false,
      hasLoadedOnce: true,
      total: 0,
      entryCount: 0,
      scanRunning: false,
    }),
    'empty',
  );
});

test('resolveLibraryDisplay never shows empty while initial load still pending', () => {
  assert.equal(
    resolveLibraryDisplay({
      initialLoading: true,
      hasLoadedOnce: false,
      total: 0,
      entryCount: 0,
      scanRunning: false,
    }),
    'loading',
  );
  assert.equal(
    resolveLibraryDisplay({
      initialLoading: true,
      hasLoadedOnce: false,
      total: 0,
      entryCount: 0,
      scanRunning: true,
    }),
    'indexing',
  );
});

test('resolveLibraryDisplay keeps grid during refresh even when total changed', () => {
  assert.equal(
    resolveLibraryDisplay({
      initialLoading: false,
      hasLoadedOnce: true,
      total: 10,
      entryCount: 5,
      scanRunning: false,
    }),
    'grid',
  );
});
