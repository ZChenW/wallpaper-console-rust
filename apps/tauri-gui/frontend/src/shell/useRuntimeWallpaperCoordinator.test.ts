import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

test('SinglePageShell consumes one runtime coordinator instead of runtime internals', async () => {
  const shell = await readFile(
    new URL('./SinglePageShell.tsx', import.meta.url),
    'utf8',
  );

  assert.match(shell, /useRuntimeWallpaperCoordinator/);
  assert.doesNotMatch(shell, /RuntimeObservationController/);
  assert.doesNotMatch(shell, /useApplyQueue/);
  assert.doesNotMatch(shell, /runtimeWallpaperSession/);
  assert.doesNotMatch(shell, /resolveCurrentWallpaperState/);
  assert.doesNotMatch(shell, /useReducer/);
  assert.doesNotMatch(shell, /visibilitychange/);
});
