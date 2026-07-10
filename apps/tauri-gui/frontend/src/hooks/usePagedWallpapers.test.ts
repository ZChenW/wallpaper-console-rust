import assert from 'node:assert/strict';
import test from 'node:test';

import { formatLoadPageError } from './usePagedWallpapers.ts';

test('formatLoadPageError preserves Error message text', () => {
  assert.equal(formatLoadPageError(new Error('database is locked')), 'database is locked');
});

test('formatLoadPageError preserves string errors', () => {
  assert.equal(formatLoadPageError('database is locked'), 'database is locked');
});

test('formatLoadPageError falls back for unknown error shapes', () => {
  assert.equal(formatLoadPageError({ code: 1 }), 'Failed to load library page');
});
