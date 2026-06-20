import assert from 'node:assert/strict';
import test from 'node:test';

import {
  calculateColumnCount,
  overscanRowsFor,
} from './layout.ts';

test('calculateColumnCount keeps invalid widths at one column', () => {
  assert.equal(calculateColumnCount(0), 1);
  assert.equal(calculateColumnCount(-20), 1);
  assert.equal(calculateColumnCount(Number.NaN), 1);
  assert.equal(calculateColumnCount(Number.POSITIVE_INFINITY), 1);
});

test('calculateColumnCount scales across compact and wide layouts', () => {
  assert.equal(calculateColumnCount(390), 1);
  assert.equal(calculateColumnCount(900), 3);
  assert.equal(calculateColumnCount(1440), 6);
  assert.equal(calculateColumnCount(1920), 8);
});

test('calculateColumnCount scales on ultra-wide layouts without cap', () => {
  assert.equal(calculateColumnCount(2560), 11);
  assert.equal(calculateColumnCount(3840), 16);
});

test('overscanRowsFor uses fewer rows while scrolling fast', () => {
  assert.equal(overscanRowsFor(8, false), 2);
  assert.equal(overscanRowsFor(8, true), 1);
});

test('overscanRowsFor scales down rows as column count grows', () => {
  assert.equal(overscanRowsFor(16, false), 1);
  assert.equal(overscanRowsFor(16, true), 1);
  assert.ok(overscanRowsFor(16, false) <= overscanRowsFor(8, false));
});
