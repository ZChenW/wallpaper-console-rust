import assert from 'node:assert/strict';
import test from 'node:test';

import { trapDialogFocus, wrappedDialogFocusIndex } from './dialogFocus.ts';

test('dialog focus wraps only at its keyboard boundaries', () => {
  assert.equal(wrappedDialogFocusIndex(0, 4, true), 3);
  assert.equal(wrappedDialogFocusIndex(3, 4, false), 0);
  assert.equal(wrappedDialogFocusIndex(1, 4, false), null);
  assert.equal(wrappedDialogFocusIndex(2, 4, true), null);
});

test('dialog focus enters from outside and safely handles empty dialogs', () => {
  assert.equal(wrappedDialogFocusIndex(-1, 3, false), 0);
  assert.equal(wrappedDialogFocusIndex(-1, 3, true), 2);
  assert.equal(wrappedDialogFocusIndex(-1, 0, false), null);
});

test('dialog focus ignores roving controls with negative tabIndex when wrapping Tab', () => {
  let prevented = 0;
  let closeFocused = 0;
  const element = (tabIndex: number, focus = () => undefined) => ({
    hidden: false,
    tabIndex,
    getAttribute: () => null,
    closest: () => null,
    getClientRects: () => ({ length: 1 }),
    focus,
  });
  const close = element(0, () => { closeFocused += 1; });
  const active = element(0);
  const rovingOverscan = element(-1);
  const event = {
    key: 'Tab',
    shiftKey: false,
    preventDefault: () => { prevented += 1; },
  } as unknown as Parameters<typeof trapDialogFocus>[0];
  const dialog = {
    querySelectorAll: () => [close, active, rovingOverscan],
  } as unknown as Parameters<typeof trapDialogFocus>[1];
  const previousDocument = Object.getOwnPropertyDescriptor(globalThis, 'document');

  Object.defineProperty(globalThis, 'document', {
    configurable: true,
    value: { activeElement: active },
  });
  try {
    trapDialogFocus(event, dialog);
  } finally {
    if (previousDocument) Object.defineProperty(globalThis, 'document', previousDocument);
    else Reflect.deleteProperty(globalThis, 'document');
  }

  assert.equal(prevented, 1);
  assert.equal(closeFocused, 1);
});
