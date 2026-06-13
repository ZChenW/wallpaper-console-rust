import assert from 'node:assert/strict';
import test from 'node:test';

import { calculateColumnCount } from './layout.ts';

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
