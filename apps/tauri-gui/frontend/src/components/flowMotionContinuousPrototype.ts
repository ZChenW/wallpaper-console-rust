export interface ContinuousFlowVisualState {
  readonly focus: number;
  readonly opacity: number;
  readonly scale: number;
  readonly grayscale: number;
  readonly contrast: number;
  readonly mediaOpacity: number;
}

export interface ContinuousFlowPositions {
  readonly viewportCenter: number;
  readonly itemCenters: readonly number[];
  readonly normalizationDistance: number;
}

export interface ApplyContinuousFlowMotionOptions {
  /**
   * Distance from the viewport center at which an item reaches its resting
   * visual state. Defaults to the median distance between rendered items.
   */
  readonly normalizationDistance?: number;
}

const RESTING_VISUAL_STATE = {
  opacity: 0.56,
  scale: 0.97,
  grayscale: 0.55,
  contrast: 1.04,
  mediaOpacity: 0.9,
} as const;

const FOCUSED_VISUAL_STATE = {
  opacity: 1,
  scale: 1,
  grayscale: 0,
  contrast: 1,
  mediaOpacity: 1,
} as const;

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}

function lerp(from: number, to: number, progress: number): number {
  return from + (to - from) * progress;
}

/**
 * Smoothstep on the unit interval. Its zero slope at both ends avoids a
 * visible velocity change when a card enters or leaves the focus range.
 */
export function smoothstep01(value: number): number {
  const clamped = clamp01(value);
  return clamped * clamped * (3 - 2 * clamped);
}

export function resolveContinuousFlowFocus(
  viewportCenter: number,
  itemCenter: number,
  normalizationDistance: number,
): number {
  if (!Number.isFinite(viewportCenter) || !Number.isFinite(itemCenter)) return 0;
  if (!Number.isFinite(normalizationDistance) || normalizationDistance <= 0) return 0;

  const linearFocus = 1 - Math.abs(itemCenter - viewportCenter) / normalizationDistance;
  return smoothstep01(linearFocus);
}

export function resolveContinuousFlowVisualState(
  focus: number,
): ContinuousFlowVisualState {
  const clampedFocus = clamp01(focus);
  return {
    focus: clampedFocus,
    opacity: lerp(RESTING_VISUAL_STATE.opacity, FOCUSED_VISUAL_STATE.opacity, clampedFocus),
    scale: lerp(RESTING_VISUAL_STATE.scale, FOCUSED_VISUAL_STATE.scale, clampedFocus),
    grayscale: lerp(
      RESTING_VISUAL_STATE.grayscale,
      FOCUSED_VISUAL_STATE.grayscale,
      clampedFocus,
    ),
    contrast: lerp(
      RESTING_VISUAL_STATE.contrast,
      FOCUSED_VISUAL_STATE.contrast,
      clampedFocus,
    ),
    mediaOpacity: lerp(
      RESTING_VISUAL_STATE.mediaOpacity,
      FOCUSED_VISUAL_STATE.mediaOpacity,
      clampedFocus,
    ),
  };
}

export function resolveContinuousFlowVisualStates({
  viewportCenter,
  itemCenters,
  normalizationDistance,
}: ContinuousFlowPositions): ContinuousFlowVisualState[] {
  return itemCenters.map((itemCenter) => resolveContinuousFlowVisualState(
    resolveContinuousFlowFocus(viewportCenter, itemCenter, normalizationDistance),
  ));
}

export function inferContinuousFlowNormalizationDistance(
  itemCenters: readonly number[],
  fallbackDistance: number,
): number {
  const distances: number[] = [];
  for (let index = 1; index < itemCenters.length; index += 1) {
    const distance = Math.abs(itemCenters[index]! - itemCenters[index - 1]!);
    if (Number.isFinite(distance) && distance > 0) distances.push(distance);
  }
  distances.sort((left, right) => left - right);

  if (distances.length > 0) {
    const midpoint = Math.floor(distances.length / 2);
    if (distances.length % 2 === 1) return distances[midpoint]!;
    return (distances[midpoint - 1]! + distances[midpoint]!) / 2;
  }

  return Number.isFinite(fallbackDistance) && fallbackDistance > 0
    ? fallbackDistance
    : 1;
}

function setStylePropertyIfChanged(
  style: CSSStyleDeclaration,
  property: string,
  value: number,
): void {
  const serialized = String(Number(value.toFixed(5)));
  if (style.getPropertyValue(property) !== serialized) {
    style.setProperty(property, serialized);
  }
}

/**
 * Applies the continuous prototype directly to the currently rendered Flow
 * items. All geometry reads happen before the CSS-variable writes, so calling
 * this from a scroll animation frame does not alternate layout reads/writes or
 * update React state.
 */
export function applyContinuousFlowMotionPrototype(
  viewport: HTMLElement,
  options: ApplyContinuousFlowMotionOptions = {},
): void {
  const items = Array.from(
    viewport.querySelectorAll<HTMLElement>('.flow-preview-item'),
  );
  if (items.length === 0) return;

  const viewportBounds = viewport.getBoundingClientRect();
  const viewportCenter = viewportBounds.top + viewportBounds.height / 2;
  const itemCenters = items.map((item) => {
    const bounds = item.getBoundingClientRect();
    return bounds.top + bounds.height / 2;
  });
  const normalizationDistance = options.normalizationDistance
    ?? inferContinuousFlowNormalizationDistance(
      itemCenters,
      Math.max(1, viewportBounds.height * 0.6),
    );
  const states = resolveContinuousFlowVisualStates({
    viewportCenter,
    itemCenters,
    normalizationDistance,
  });

  items.forEach((item, index) => {
    const state = states[index]!;
    setStylePropertyIfChanged(item.style, '--flow-motion-focus', state.focus);
    setStylePropertyIfChanged(item.style, '--flow-motion-opacity', state.opacity);
    setStylePropertyIfChanged(item.style, '--flow-motion-scale', state.scale);
    setStylePropertyIfChanged(item.style, '--flow-motion-grayscale', state.grayscale);
    setStylePropertyIfChanged(item.style, '--flow-motion-contrast', state.contrast);
    setStylePropertyIfChanged(
      item.style,
      '--flow-motion-media-opacity',
      state.mediaOpacity,
    );
  });

}

export function clearContinuousFlowMotionPrototype(viewport: HTMLElement): void {
  for (const item of viewport.querySelectorAll<HTMLElement>('.flow-preview-item')) {
    item.style.removeProperty('--flow-motion-focus');
    item.style.removeProperty('--flow-motion-opacity');
    item.style.removeProperty('--flow-motion-scale');
    item.style.removeProperty('--flow-motion-grayscale');
    item.style.removeProperty('--flow-motion-contrast');
    item.style.removeProperty('--flow-motion-media-opacity');
  }
}
