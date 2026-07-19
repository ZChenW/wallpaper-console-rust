import assert from 'node:assert/strict';
import test from 'node:test';

import { createRecurringErrorGate } from './recurringErrorGate.ts';

test('recurring error gate notifies again after recovery', () => {
  const gate = createRecurringErrorGate();

  assert.deepEqual(
    [
      gate.shouldNotify('A'),
      gate.shouldNotify('A'),
      gate.shouldNotify(null),
      gate.shouldNotify('A'),
    ],
    [true, false, false, true],
  );
});
