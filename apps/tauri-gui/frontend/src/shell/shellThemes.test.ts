import assert from 'node:assert/strict';
import test from 'node:test';

import {
  isShellTheme,
  nativeWindowTheme,
  SHELL_THEME_OPTIONS,
} from './shellThemes.ts';

test('theme registry owns unique persisted values and visible labels', () => {
  assert.deepEqual(SHELL_THEME_OPTIONS, [
    { value: 'system', label: 'System' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
    { value: 'glass', label: 'Glass' },
    { value: 'editorial', label: 'Editorial' },
  ]);
  assert.equal(new Set(SHELL_THEME_OPTIONS.map(({ value }) => value)).size, 5);
  assert.equal(new Set(SHELL_THEME_OPTIONS.map(({ label }) => label)).size, 5);
  assert.equal(isShellTheme('editorial'), true);
  assert.equal(isShellTheme('obsidian_warm'), false);
});

test('custom themes map to supported native window chrome', () => {
  assert.equal(nativeWindowTheme('system'), null);
  assert.equal(nativeWindowTheme('light'), 'light');
  assert.equal(nativeWindowTheme('dark'), 'dark');
  assert.equal(nativeWindowTheme('glass'), 'dark');
  assert.equal(nativeWindowTheme('editorial'), 'light');
});
