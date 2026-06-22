import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { normalizeConfigValue } from './configNormalizer.ts';

const schemaPath = join(dirname(fileURLToPath(import.meta.url)), 'configSchema.ts');

test('settings schema does not expose unused Wallpaper Engine assets dir', () => {
  const source = readFileSync(schemaPath, 'utf8');
  const assetsDirKey = ['linux', 'wallpaperengine', 'assets', 'dir'].join('_');

  assert.equal(source.includes(`key: '${assetsDirKey}'`), false);
});

test('library settings do not expose inactive scan filter keys', () => {
  const source = readFileSync(schemaPath, 'utf8');
  const minWidthKey = ['min', 'wallpaper', 'width'].join('_');
  const minHeightKey = ['min', 'wallpaper', 'height'].join('_');

  assert.equal(source.includes(`key: '${minWidthKey}'`), false);
  assert.equal(source.includes(`key: '${minHeightKey}'`), false);
});

test('gui_thumbnail_mode only allows cache or icon', () => {
  assert.equal(normalizeConfigValue('gui_thumbnail_mode', 'cache'), 'cache');
  assert.equal(normalizeConfigValue('gui_thumbnail_mode', 'icon'), 'icon');
  assert.equal(normalizeConfigValue('gui_thumbnail_mode', 'original'), 'cache');
  assert.equal(normalizeConfigValue('gui_thumbnail_mode', 'bad'), 'cache');
});
