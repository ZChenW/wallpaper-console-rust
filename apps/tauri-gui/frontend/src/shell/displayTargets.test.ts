import assert from 'node:assert/strict';
import test from 'node:test';

import {
  ALL_DISPLAYS_SELECT_VALUE,
  buildDisplayTargetModel,
  displayTargetFromSelectValue,
  displayTargetToSelectValue,
  normalizeDisplayOutputs,
  resolveConnectedDisplayTarget,
} from './displayTargets.ts';

test('display output names are trimmed, deduplicated, and keep discovery order', () => {
  assert.deepEqual(
    normalizeDisplayOutputs(['  eDP-1 ', '', 'HDMI-A-1', 'eDP-1', '  ', 'DP-2']),
    ['eDP-1', 'HDMI-A-1', 'DP-2'],
  );
});

test('selector model is hidden with zero or one connected output', () => {
  assert.equal(buildDisplayTargetModel([], { kind: 'allDisplays' }).hidden, true);
  assert.equal(
    buildDisplayTargetModel(['eDP-1'], { kind: 'output', output: 'eDP-1' }).hidden,
    true,
  );
});

test('selector model offers All Displays followed by every connected output', () => {
  const model = buildDisplayTargetModel(
    [' eDP-1 ', 'HDMI-A-1', 'eDP-1'],
    { kind: 'output', output: ' HDMI-A-1 ' },
  );

  assert.equal(model.hidden, false);
  assert.deepEqual(model.connectedOutputs, ['eDP-1', 'HDMI-A-1']);
  assert.deepEqual(model.selectedTarget, { kind: 'output', output: 'HDMI-A-1' });
  assert.deepEqual(model.options, [
    { label: 'All Displays', value: ALL_DISPLAYS_SELECT_VALUE },
    { label: 'eDP-1', value: displayTargetToSelectValue({ kind: 'output', output: 'eDP-1' }) },
    {
      label: 'HDMI-A-1',
      value: displayTargetToSelectValue({ kind: 'output', output: 'HDMI-A-1' }),
    },
  ]);
});

test('blank or disconnected saved output falls back conservatively to All Displays', () => {
  assert.deepEqual(
    resolveConnectedDisplayTarget({ kind: 'output', output: '   ' }, ['eDP-1']),
    { kind: 'allDisplays' },
  );
  assert.deepEqual(
    resolveConnectedDisplayTarget({ kind: 'output', output: 'DP-9' }, ['eDP-1']),
    { kind: 'allDisplays' },
  );
  assert.deepEqual(
    buildDisplayTargetModel(['eDP-1', 'HDMI-A-1'], { kind: 'output', output: 'DP-9' })
      .selectedTarget,
    { kind: 'allDisplays' },
  );
});

test('select values reversibly encode All Displays and arbitrary output names', () => {
  const named = { kind: 'output' as const, output: ' DP: 2 / 工作区 ' };
  const normalizedNamed = { kind: 'output' as const, output: 'DP: 2 / 工作区' };

  assert.deepEqual(
    displayTargetFromSelectValue(displayTargetToSelectValue({ kind: 'allDisplays' })),
    { kind: 'allDisplays' },
  );
  assert.deepEqual(
    displayTargetFromSelectValue(displayTargetToSelectValue(named)),
    normalizedNamed,
  );
  assert.deepEqual(displayTargetFromSelectValue('output:%not-valid'), { kind: 'allDisplays' });
  assert.deepEqual(displayTargetFromSelectValue('unknown'), { kind: 'allDisplays' });
});
