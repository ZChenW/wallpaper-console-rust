import assert from 'node:assert/strict';
import test from 'node:test';

import {
  DEFAULT_SHELL_PREFERENCES,
  parseShellPreferences,
  serializeShellPreferences,
  type ShellPreferences,
} from './shellPreferences.ts';

test('shell preferences default to the direct single-click library experience', () => {
  assert.deepEqual(parseShellPreferences(null), DEFAULT_SHELL_PREFERENCES);
  assert.deepEqual(DEFAULT_SHELL_PREFERENCES, {
    sourceFilter: { kind: 'all' },
    typeFilter: 'usable',
    favoritesOnly: false,
    sort: 'recentlyAdded',
    cardSize: 'medium',
    displayTarget: { kind: 'allDisplays' },
    applyGesture: 'single',
    theme: 'system',
    libraryViewMode: 'grid',
  });
});

test('shell preferences serialize and parse every remembered value', () => {
  const preferences: ShellPreferences = {
    sourceFilter: { kind: 'source', sourceId: 42 },
    typeFilter: 'weScene',
    favoritesOnly: true,
    sort: 'nameDesc',
    cardSize: 'large',
    displayTarget: { kind: 'output', output: 'DP-2' },
    applyGesture: 'double',
    theme: 'dark',
    libraryViewMode: 'flow',
  };

  assert.deepEqual(parseShellPreferences(serializeShellPreferences(preferences)), preferences);
});

test('shell preferences repair malformed JSON and unknown legacy values field by field', () => {
  assert.deepEqual(parseShellPreferences('{broken'), DEFAULT_SHELL_PREFERENCES);
  assert.deepEqual(
    parseShellPreferences(JSON.stringify({
      sourceFilter: { kind: 'source', sourceId: -3 },
      typeFilter: 'all',
      favoritesOnly: 'true',
      sort: 'newest',
      cardSize: 'huge',
      displayTarget: { kind: 'output', output: '  ' },
      applyGesture: 'click',
      theme: 'obsidian_warm',
      libraryViewMode: 'masonry',
    })),
    DEFAULT_SHELL_PREFERENCES,
  );

  assert.deepEqual(
    parseShellPreferences(JSON.stringify({
      typeFilter: 'video',
      favoritesOnly: true,
      sort: 'nameAsc',
      theme: 'light',
      futureField: 'ignored',
    })),
    {
      ...DEFAULT_SHELL_PREFERENCES,
      typeFilter: 'video',
      favoritesOnly: true,
      sort: 'nameAsc',
      theme: 'light',
    },
  );
});

test('library view mode repairs missing and unknown persisted values to grid', () => {
  assert.equal(
    parseShellPreferences(JSON.stringify({ theme: 'dark' })).libraryViewMode,
    'grid',
  );
  assert.equal(
    parseShellPreferences(JSON.stringify({ libraryViewMode: 'masonry' })).libraryViewMode,
    'grid',
  );
});

test('double-click apply is opt-in and survives persistence', () => {
  assert.equal(DEFAULT_SHELL_PREFERENCES.applyGesture, 'single');
  assert.equal(
    parseShellPreferences(serializeShellPreferences({
      ...DEFAULT_SHELL_PREFERENCES,
      applyGesture: 'double',
    })).applyGesture,
    'double',
  );
});

test('glass theme survives persistence', () => {
  assert.equal(
    parseShellPreferences(serializeShellPreferences({
      ...DEFAULT_SHELL_PREFERENCES,
      theme: 'glass',
    })).theme,
    'glass',
  );
});

test('editorial theme survives persistence', () => {
  assert.equal(
    parseShellPreferences(serializeShellPreferences({
      ...DEFAULT_SHELL_PREFERENCES,
      theme: 'editorial',
    })).theme,
    'editorial',
  );
});

test('serialization excludes transient shell state even when extra properties reach runtime', () => {
  const runtimeState = {
    ...DEFAULT_SHELL_PREFERENCES,
    search: 'must not return',
    selectedPath: '/walls/secret.jpg',
    scrollTop: 820,
    flowCenteredWallpaperId: 'wallpaper-42',
    flowScrollTop: 1640,
    scanProgress: { scanned: 9 },
    feedback: { message: 'done' },
  };

  const encoded = serializeShellPreferences(runtimeState);
  const value = JSON.parse(encoded) as Record<string, unknown>;

  assert.deepEqual(Object.keys(value).sort(), [
    'applyGesture',
    'cardSize',
    'displayTarget',
    'favoritesOnly',
    'libraryViewMode',
    'sort',
    'sourceFilter',
    'theme',
    'typeFilter',
    'version',
  ]);
  assert.equal('search' in value, false);
  assert.equal('selectedPath' in value, false);
  assert.equal('scrollTop' in value, false);
  assert.equal('flowCenteredWallpaperId' in value, false);
  assert.equal('flowScrollTop' in value, false);
  assert.equal('scanProgress' in value, false);
  assert.equal('feedback' in value, false);
});

test('output targets are trimmed and non-finite source ids are rejected', () => {
  assert.deepEqual(
    parseShellPreferences(JSON.stringify({
      sourceFilter: { kind: 'source', sourceId: Number.POSITIVE_INFINITY },
      displayTarget: { kind: 'output', output: '  HDMI-A-1  ' },
    })),
    {
      ...DEFAULT_SHELL_PREFERENCES,
      displayTarget: { kind: 'output', output: 'HDMI-A-1' },
    },
  );
});
