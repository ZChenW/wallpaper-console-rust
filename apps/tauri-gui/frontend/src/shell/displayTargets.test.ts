import assert from 'node:assert/strict';
import test from 'node:test';

import {
  ALL_DISPLAYS_SELECT_VALUE,
  buildDisplayTargetModel,
  displayTargetFromSelectValue,
  displayTargetToSelectValue,
  normalizeDisplayTarget,
  normalizeDisplayOutputs,
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
  assert.equal(
    buildDisplayTargetModel([], { kind: 'output', output: 'DP-9' }).hidden,
    false,
    'a disconnected saved target must remain visible so the user can repair it',
  );
});

test('All Displays can apply only when at least one output is connected', () => {
  assert.equal(
    buildDisplayTargetModel([], { kind: 'allDisplays' }).canApply,
    false,
  );
  assert.equal(
    buildDisplayTargetModel(['eDP-1'], { kind: 'allDisplays' }).canApply,
    true,
  );
  assert.equal(
    buildDisplayTargetModel(['eDP-1', 'HDMI-A-1'], { kind: 'allDisplays' }).canApply,
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
  assert.equal(model.canApply, true);
  assert.deepEqual(model.options, [
    { label: 'All Displays', value: ALL_DISPLAYS_SELECT_VALUE, disabled: false },
    {
      label: 'eDP-1',
      value: displayTargetToSelectValue({ kind: 'output', output: 'eDP-1' }),
      disabled: false,
    },
    {
      label: 'HDMI-A-1',
      value: displayTargetToSelectValue({ kind: 'output', output: 'HDMI-A-1' }),
      disabled: false,
    },
  ]);
});

test('blank saved output normalizes to All Displays', () => {
  assert.deepEqual(
    normalizeDisplayTarget({ kind: 'output', output: '   ' }),
    { kind: 'allDisplays' },
  );
});

test('disconnected saved output stays selected, unavailable, and never encodes as All Displays', () => {
  const saved = { kind: 'output' as const, output: ' DP-9 ' };
  const resolved = normalizeDisplayTarget(saved);
  const model = buildDisplayTargetModel(['eDP-1', 'HDMI-A-1'], saved);

  assert.deepEqual(resolved, { kind: 'output', output: 'DP-9' });
  assert.deepEqual(model.selectedTarget, { kind: 'output', output: 'DP-9' });
  assert.equal(model.canApply, false);
  assert.notEqual(displayTargetToSelectValue(model.selectedTarget), ALL_DISPLAYS_SELECT_VALUE);
  assert.deepEqual(model.options.at(-1), {
    label: 'DP-9 (Disconnected)',
    value: displayTargetToSelectValue({ kind: 'output', output: 'DP-9' }),
    disabled: true,
  });
});

test('disconnected selection changes only after an explicit valid choice', () => {
  const saved = { kind: 'output' as const, output: 'DP-9' };
  const beforeChange = buildDisplayTargetModel(['eDP-1'], saved);
  const explicitlyAll = buildDisplayTargetModel(
    ['eDP-1'],
    displayTargetFromSelectValue(ALL_DISPLAYS_SELECT_VALUE),
  );
  const explicitlyConnected = buildDisplayTargetModel(
    ['eDP-1'],
    displayTargetFromSelectValue(
      displayTargetToSelectValue({ kind: 'output', output: 'eDP-1' }),
    ),
  );

  assert.deepEqual(beforeChange.selectedTarget, saved);
  assert.equal(beforeChange.canApply, false);
  assert.deepEqual(explicitlyAll.selectedTarget, { kind: 'allDisplays' });
  assert.deepEqual(explicitlyConnected.selectedTarget, { kind: 'output', output: 'eDP-1' });
  assert.deepEqual(saved, { kind: 'output', output: 'DP-9' });
});

test('reconnected saved output becomes available without changing its identity', () => {
  const model = buildDisplayTargetModel(
    ['eDP-1', 'DP-9'],
    { kind: 'output', output: 'DP-9' },
  );

  assert.deepEqual(model.selectedTarget, { kind: 'output', output: 'DP-9' });
  assert.equal(model.canApply, true);
  assert.deepEqual(model.options.at(-1), {
    label: 'DP-9',
    value: displayTargetToSelectValue({ kind: 'output', output: 'DP-9' }),
    disabled: false,
  });
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
