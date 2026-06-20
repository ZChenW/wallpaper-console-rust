import assert from 'node:assert/strict';
import test from 'node:test';

import {
  resolveDatabaseStatusCard,
  resolveThumbnailStatusCard,
  resolveWeStatusCard,
} from './statusCards.ts';

test('resolveDatabaseStatusCard shows Checking while loading', () => {
  const card = resolveDatabaseStatusCard(null, null, true);
  assert.equal(card.value, 'Checking...');
  assert.equal(card.tone, 'neutral');
});

test('resolveDatabaseStatusCard shows Unavailable on error', () => {
  const card = resolveDatabaseStatusCard(null, 'db offline', false);
  assert.equal(card.value, 'Unavailable');
  assert.equal(card.detail, 'db offline');
  assert.equal(card.tone, 'warning');
});

test('resolveDatabaseStatusCard shows indexed count when ready', () => {
  const card = resolveDatabaseStatusCard(
    {
      configured: '/data',
      effective: '/data',
      sqliteReady: true,
      sqliteRows: 42,
      tsvRows: 0,
      stale: false,
      message: '',
    },
    null,
    false,
  );
  assert.equal(card.value, '42 wallpapers indexed');
});

test('resolveWeStatusCard does not show Missing while loading or null', () => {
  assert.equal(resolveWeStatusCard(null, null, true).value, 'Checking...');
  assert.equal(resolveWeStatusCard(null, null, false).value, 'Checking...');
  assert.notEqual(resolveWeStatusCard(null, null, false).value, 'Missing');
});

test('resolveWeStatusCard shows Unavailable on error', () => {
  const card = resolveWeStatusCard(null, 'ipc failed', false);
  assert.equal(card.value, 'Unavailable');
  assert.equal(card.detail, 'ipc failed');
  assert.equal(card.tone, 'warning');
});

test('resolveWeStatusCard shows Ready when available', () => {
  const card = resolveWeStatusCard(
    { available: true, path: '/opt/we', message: 'ok' },
    null,
    false,
  );
  assert.equal(card.value, 'Ready — /opt/we');
  assert.equal(card.tone, 'success');
});

test('resolveWeStatusCard shows Missing only when fulfilled unavailable', () => {
  const card = resolveWeStatusCard(
    { available: false, message: 'linux-wallpaperengine not found' },
    null,
    false,
  );
  assert.equal(card.value, 'Missing');
  assert.equal(card.detail, 'linux-wallpaperengine not found');
});

test('resolveThumbnailStatusCard shows Checking while loading', () => {
  const card = resolveThumbnailStatusCard(null, null, true);
  assert.equal(card.value, 'Checking...');
});

test('resolveThumbnailStatusCard shows Unavailable on error', () => {
  const card = resolveThumbnailStatusCard(null, 'cache unreadable', false);
  assert.equal(card.value, 'Unavailable');
  assert.equal(card.detail, 'cache unreadable');
  assert.equal(card.tone, 'warning');
});

test('resolveThumbnailStatusCard shows cache stats when ready', () => {
  const card = resolveThumbnailStatusCard(
    {
      dir: '/tmp/thumbs',
      size: '12 MB',
      entries: 8,
      failureEntries: 0,
      cleanupDays: 30,
    },
    null,
    false,
    { cleanupDays: 14 },
  );
  assert.equal(card.value, '8 thumbnails, 12 MB');
  assert.equal(card.detail, 'Cleanup: older than 14 days');
});
