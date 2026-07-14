import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const tauriConfigUrl = new URL('../../../src-tauri/tauri.conf.json', import.meta.url);
const capabilitiesUrl = new URL('../../../src-tauri/capabilities/default.json', import.meta.url);
const shellUrl = new URL('./SinglePageShell.tsx', import.meta.url);

test('main window is frameless and grants only the required drag permission', async () => {
  const [tauriConfigSource, capabilitiesSource] = await Promise.all([
    readFile(tauriConfigUrl, 'utf8'),
    readFile(capabilitiesUrl, 'utf8'),
  ]);
  const tauriConfig = JSON.parse(tauriConfigSource) as {
    app?: { windows?: Array<{ decorations?: boolean }> };
  };
  const capabilities = JSON.parse(capabilitiesSource) as { permissions?: string[] };

  assert.equal(tauriConfig.app?.windows?.[0]?.decorations, false);
  assert.deepEqual(capabilities.permissions, [
    'core:default',
    'core:window:allow-start-dragging',
  ]);
});

test('existing topbar is a deep Tauri drag surface', async () => {
  const shellSource = await readFile(shellUrl, 'utf8');

  assert.match(
    shellSource,
    /<header\s+className="single-page-topbar"\s+data-tauri-drag-region="deep">/,
  );
});
