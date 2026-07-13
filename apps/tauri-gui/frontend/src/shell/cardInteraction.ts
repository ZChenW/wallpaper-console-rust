import type { ApplyGesture } from './shellPreferences.ts';

export interface CardPointerInteractionInput {
  readonly gesture: ApplyGesture;
  /** Browser MouseEvent.detail: 1 for the first click, 2 for the second. */
  readonly clickCount: number;
  readonly canApply: boolean;
  /** True when the event originated in a button/menu/control inside the card. */
  readonly fromControl: boolean;
}

export interface CardPointerInteraction {
  readonly select: boolean;
  readonly apply: boolean;
}

export interface CardInteractionVisualState {
  readonly selected: boolean;
  readonly pending: boolean;
  readonly current: boolean;
}

const NONE: CardPointerInteraction = Object.freeze({ select: false, apply: false });

/**
 * Convert one card click event into selection/application intent.
 *
 * In single-click mode the second event from a physical double click is ignored,
 * preventing duplicate apply requests. In double-click mode the first event only
 * selects and the second event applies. Embedded controls own their events and
 * can never activate the card.
 */
export function resolveCardPointerInteraction(
  input: CardPointerInteractionInput,
): CardPointerInteraction {
  if (
    input.fromControl
    || !Number.isSafeInteger(input.clickCount)
    || !matchesHandledClick(input.gesture, input.clickCount)
  ) {
    return NONE;
  }

  const shouldApply = input.canApply
    && (
      (input.gesture === 'single' && input.clickCount === 1)
      || (input.gesture === 'double' && input.clickCount === 2)
    );
  return { select: true, apply: shouldApply };
}

function matchesHandledClick(gesture: ApplyGesture, clickCount: number): boolean {
  return gesture === 'single' ? clickCount === 1 : clickCount === 1 || clickCount === 2;
}

export function cardInteractionClassName(state: CardInteractionVisualState): string {
  return [
    'wallpaper-card',
    state.selected ? 'selected' : '',
    state.pending ? 'pending' : '',
    state.current ? 'current' : '',
  ].filter(Boolean).join(' ');
}
