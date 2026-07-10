import assert from 'node:assert/strict';
import test from 'node:test';

import {
  calculateGridLayout,
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

test('calculateGridLayout returns fixed pixel row snapshot for measured width', () => {
  assert.deepEqual(calculateGridLayout(900), {
    colCount: 3,
    columnWidth: 293.3333333333333,
    rowWidth: 900,
  });
  assert.deepEqual(calculateGridLayout(1920), {
    colCount: 8,
    columnWidth: 231.25,
    rowWidth: 1920,
  });
});

test('calculateGridLayout keeps an invalid width usable before first measurement', () => {
  assert.deepEqual(calculateGridLayout(0), {
    colCount: 1,
    columnWidth: 220,
    rowWidth: 220,
  });
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
