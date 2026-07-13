import assert from 'node:assert/strict';
import test from 'node:test';

import {
  applyShellThemeToSurface,
  resolveShellTheme,
} from './useShellTheme.ts';

test('system theme follows the current color-scheme preference', () => {
  assert.equal(resolveShellTheme('system', false), 'light');
  assert.equal(resolveShellTheme('system', true), 'dark');
  assert.equal(resolveShellTheme('light', true), 'light');
  assert.equal(resolveShellTheme('dark', false), 'dark');
});

test('theme application updates document palette and delegates system window chrome', async () => {
  const events: unknown[][] = [];
  await applyShellThemeToSurface('system', true, {
    setDocumentTheme: (theme) => events.push(['document', theme]),
    setWindowTheme: async (theme) => { events.push(['window', theme]); },
  });

  assert.deepEqual(events, [
    ['document', 'dark'],
    ['window', null],
  ]);
});

test('missing Tauri window chrome does not prevent document theming', async () => {
  const documentThemes: string[] = [];
  await applyShellThemeToSurface('dark', false, {
    setDocumentTheme: (theme) => documentThemes.push(theme),
    setWindowTheme: async () => { throw new Error('not in Tauri'); },
  });
  assert.deepEqual(documentThemes, ['dark']);
});
