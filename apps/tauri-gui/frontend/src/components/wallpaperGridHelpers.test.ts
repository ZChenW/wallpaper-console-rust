import assert from 'node:assert/strict';
import test from 'node:test';

import { shouldResetScroll } from './wallpaperGridHelpers.ts';

test('shouldResetScroll returns false when resetKey is unchanged (loadMore append)', () => {
  assert.equal(shouldResetScroll('all|newest|', 'all|newest|'), false);
});

test('shouldResetScroll returns true when resetKey changes (filter/sort/search)', () => {
  assert.equal(shouldResetScroll('all|newest|', 'image|newest|'), true);
  assert.equal(shouldResetScroll('all|newest|', 'all|name|'), true);
  assert.equal(shouldResetScroll('all|newest|', 'all|newest|foo'), true);
});

test('shouldResetScroll treats undefined transitions correctly', () => {
  assert.equal(shouldResetScroll(undefined, 'k'), true);
  assert.equal(shouldResetScroll('k', undefined), true);
  assert.equal(shouldResetScroll(undefined, undefined), false);
});
