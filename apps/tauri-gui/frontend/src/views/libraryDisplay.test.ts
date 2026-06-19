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
    loadError: false,
    emptyConfirmed: true,
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
      loadError: false,
      emptyConfirmed: false,
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
      loadError: false,
      emptyConfirmed: false,
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
      loadError: false,
      emptyConfirmed: false,
    }),
    'indexing',
  );
});

test('resolveLibraryDisplay shows empty only after confirmed zero with no scan', () => {
  assert.equal(
    resolveLibraryDisplay({
      initialLoading: false,
      hasLoadedOnce: true,
      total: 0,
      entryCount: 0,
      scanRunning: false,
      loadError: false,
      emptyConfirmed: true,
    }),
    'empty',
  );
});

test('resolveLibraryDisplay shows loading for unconfirmed first zero (no empty flash)', () => {
  assert.equal(
    resolveLibraryDisplay({
      initialLoading: false,
      hasLoadedOnce: true,
      total: 0,
      entryCount: 0,
      scanRunning: false,
      loadError: false,
      emptyConfirmed: false,
    }),
    'loading',
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
      loadError: false,
      emptyConfirmed: false,
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
      loadError: false,
      emptyConfirmed: false,
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
      loadError: false,
      emptyConfirmed: true,
    }),
    'grid',
  );
});

test('resolveLibraryDisplay shows error when load failed and no entries', () => {
  assert.equal(
    resolveLibraryDisplay({
      initialLoading: false,
      hasLoadedOnce: false,
      total: 0,
      entryCount: 0,
      scanRunning: false,
      loadError: true,
      emptyConfirmed: false,
    }),
    'error',
  );
  assert.equal(
    resolveLibraryDisplay({
      initialLoading: false,
      hasLoadedOnce: true,
      total: 0,
      entryCount: 0,
      scanRunning: false,
      loadError: true,
      emptyConfirmed: false,
    }),
    'error',
  );
});

test('resolveLibraryDisplay shows grid even when loadError is true if entries exist', () => {
  assert.equal(
    resolveLibraryDisplay({
      initialLoading: false,
      hasLoadedOnce: true,
      total: 50,
      entryCount: 30,
      scanRunning: false,
      loadError: true,
      emptyConfirmed: false,
    }),
    'grid',
  );
});

test('resolveLibraryDisplay prefers indexing over error when scan is running', () => {
  assert.equal(
    resolveLibraryDisplay({
      initialLoading: false,
      hasLoadedOnce: false,
      total: 0,
      entryCount: 0,
      scanRunning: true,
      loadError: true,
      emptyConfirmed: false,
    }),
    'indexing',
  );
});
