export const FLOW_SCROLL_IDLE_MS = 250;
export const FLOW_SNAP_DURATION_MS = 300;

export interface FlowKeyInput {
  readonly key: string;
  readonly currentIndex: number;
  readonly itemCount: number;
  readonly pageStep?: number;
  readonly ctrlKey?: boolean;
  readonly metaKey?: boolean;
  readonly shiftKey?: boolean;
  readonly hasMore?: boolean;
  readonly loadingMore?: boolean;
  readonly endLoadRequestAllowed?: boolean;
}

export type FlowKeyResolution =
  | {
    readonly type: 'navigate';
    readonly index: number;
    readonly requestLoadMore: boolean;
  }
  | { readonly type: 'select'; readonly index: number }
  | { readonly type: 'apply'; readonly index: number }
  | { readonly type: 'context'; readonly index: number };

export type FlowScrollBehavior = 'auto' | 'smooth';

export type FlowItemId = string | number;

export interface FlowItemGeometry<TId extends FlowItemId = FlowItemId> {
  readonly id: TId;
  readonly index: number;
  readonly start: number;
  readonly size: number;
}

export interface NearestFlowCenterInput<TId extends FlowItemId = FlowItemId> {
  readonly items: readonly FlowItemGeometry<TId>[];
  readonly viewportStart: number;
  readonly viewportSize: number;
  readonly previousCenteredId?: TId | null;
}

export interface FlowIndexRange {
  readonly startIndex: number;
  readonly endIndex: number;
}

export interface FlowIndexAlignmentInput {
  readonly railStart: number;
  readonly railSize: number;
  readonly itemStart: number;
  readonly itemSize: number;
}

export interface FlowAnchor<TId extends FlowItemId = FlowItemId> {
  readonly id: TId;
  readonly index: number;
}

export interface EstimateFlowItemSizeInput {
  readonly availableWidth: number;
  /** Width divided by height. Invalid values fall back to 16:9. */
  readonly aspectRatio?: number | null;
  readonly minHeight?: number;
  readonly maxHeight?: number;
}

export interface FlowPageStepInput {
  readonly viewportSize: number;
  readonly itemSize: number;
  readonly gap?: number;
}

export interface VisibleFlowThumbnailRangeInput {
  readonly itemCount: number;
  readonly scrollOffset: number;
  readonly viewportSize: number;
  readonly itemSize: number;
  readonly gap?: number;
  readonly overscan?: number;
}

export interface VisibleMeasuredFlowThumbnailRangeInput<
  TId extends FlowItemId = FlowItemId,
> {
  readonly items: readonly FlowItemGeometry<TId>[];
  readonly itemCount: number;
  readonly viewportStart: number;
  readonly viewportSize: number;
  readonly overscan?: number;
}

export interface FlowNextPageRequestInput {
  readonly itemCount: number;
  readonly visibleEndIndex: number | null | undefined;
  readonly hasMore: boolean;
  readonly loadingMore: boolean;
  readonly refreshing?: boolean;
  readonly automaticAppendPaused?: boolean;
  readonly threshold?: number;
}

export interface EnhancedFlowMediaEligibility {
  readonly mediaType: string;
  readonly active: boolean;
  readonly settled: boolean;
  readonly centered: boolean;
  readonly selected: boolean;
  readonly reducedMotion: boolean;
}

export interface FlowVisualState {
  readonly active: boolean;
  readonly settled: boolean;
  readonly centered: boolean;
  readonly hovered: boolean;
  readonly selected: boolean;
  readonly current: boolean;
  readonly applying: boolean;
  readonly pending: boolean;
  readonly favorite: boolean;
}

export interface FlowStatePresentation {
  readonly className: string;
  readonly attributes: {
    readonly 'data-active': true | undefined;
    readonly 'data-settled': true | undefined;
    readonly 'data-centered': true | undefined;
    readonly 'data-hovered': true | undefined;
    readonly 'data-selected': true | undefined;
    readonly 'data-current': true | undefined;
    readonly 'data-applying': true | undefined;
    readonly 'data-pending': true | undefined;
    readonly 'data-favorite': true | undefined;
  };
}

export interface FlowStateLabelInput {
  readonly selected?: boolean;
  readonly current?: boolean;
  readonly applying?: boolean;
  readonly pending?: boolean;
  readonly favorite?: boolean;
}

export interface FlowCenterIntent {
  readonly type: 'center';
  readonly index: number;
}

export interface FlowHoverIntent {
  readonly type: 'hover';
  readonly index: number;
}

export function clampFlowIndex(index: number, itemCount: number): number | null {
  const count = normalizeItemCount(itemCount);
  if (count === 0) return null;
  if (Number.isNaN(index)) return 0;
  return Math.min(count - 1, Math.max(0, Math.trunc(index)));
}

export function resolveFlowKey(input: FlowKeyInput): FlowKeyResolution | null {
  if (normalizeItemCount(input.itemCount) === 0) return null;

  const navigate = (index: number, requestLoadMore = false): FlowKeyResolution => ({
    type: 'navigate',
    index: clampFlowIndex(index, input.itemCount) ?? 0,
    requestLoadMore,
  });
  const pageStep = positiveInteger(input.pageStep, 1);

  switch (input.key) {
    case 'ArrowUp':
      return navigate(input.currentIndex - 1);
    case 'ArrowDown':
      return navigate(input.currentIndex + 1);
    case 'PageUp':
      return navigate(input.currentIndex - pageStep);
    case 'PageDown':
      return navigate(input.currentIndex + pageStep);
    case 'Home':
      return navigate(0);
    case 'End':
      return navigate(
        input.itemCount - 1,
        input.hasMore === true
          && input.loadingMore !== true
          && input.endLoadRequestAllowed === true,
      );
    case 'Enter': {
      const index = clampFlowIndex(input.currentIndex, input.itemCount) ?? 0;
      return input.ctrlKey === true || input.metaKey === true
        ? { type: 'apply', index }
        : { type: 'select', index };
    }
    case 'F10': {
      if (input.shiftKey !== true) return null;
      return {
        type: 'context',
        index: clampFlowIndex(input.currentIndex, input.itemCount) ?? 0,
      };
    }
    default:
      return null;
  }
}

export function nearestFlowCenterIndex<TId extends FlowItemId>(
  input: NearestFlowCenterInput<TId>,
): number | null {
  if (
    !Number.isFinite(input.viewportStart)
    || !Number.isFinite(input.viewportSize)
    || input.viewportSize < 0
  ) {
    return null;
  }

  const viewportCenter = input.viewportStart + input.viewportSize / 2;
  let nearest: FlowItemGeometry<TId> | null = null;
  let nearestDistance = Number.POSITIVE_INFINITY;

  for (const item of input.items) {
    if (
      !Number.isFinite(item.index)
      || !Number.isFinite(item.start)
      || !Number.isFinite(item.size)
      || item.size < 0
    ) {
      continue;
    }

    const distance = Math.abs(item.start + item.size / 2 - viewportCenter);
    const winsStableTie = distance === nearestDistance
      && Object.is(item.id, input.previousCenteredId)
      && !Object.is(nearest?.id, input.previousCenteredId);
    if (distance < nearestDistance || winsStableTie) {
      nearest = item;
      nearestDistance = distance;
    }
  }

  return nearest ? Math.trunc(nearest.index) : null;
}

export function localFlowIndexWindow(
  centerIndex: number,
  itemCount: number,
): FlowIndexRange | null {
  const center = clampFlowIndex(centerIndex, itemCount);
  if (center == null) return null;
  return {
    startIndex: Math.max(0, center - 7),
    endIndex: Math.min(normalizeItemCount(itemCount) - 1, center + 7),
  };
}

/**
 * Keeps the listbox's active option mounted when virtualization jumps to a
 * distant range. At most one offscreen index is added, so rendering remains
 * bounded by the virtualizer's normal range plus the active descendant.
 */
export function retainFlowActiveIndex(
  renderedIndexes: number[],
  activeIndex: number,
  itemCount: number,
): number[] {
  const count = normalizeItemCount(itemCount);
  if (
    !Number.isInteger(activeIndex)
    || activeIndex < 0
    || activeIndex >= count
    || renderedIndexes.includes(activeIndex)
  ) {
    return renderedIndexes;
  }

  const insertionIndex = renderedIndexes.findIndex((index) => index > activeIndex);
  if (insertionIndex < 0) return [...renderedIndexes, activeIndex];
  return [
    ...renderedIndexes.slice(0, insertionIndex),
    activeIndex,
    ...renderedIndexes.slice(insertionIndex),
  ];
}

export function flowIndexAlignmentOffset(input: FlowIndexAlignmentInput): number {
  if (
    !Number.isFinite(input.railStart)
    || !Number.isFinite(input.railSize)
    || !Number.isFinite(input.itemStart)
    || !Number.isFinite(input.itemSize)
  ) {
    return 0;
  }
  const railCenter = input.railStart + Math.max(0, input.railSize) / 2;
  const itemCenter = input.itemStart + Math.max(0, input.itemSize) / 2;
  return railCenter - itemCenter;
}

export function resolveInitialFlowAnchor<TId extends FlowItemId>(
  loadedIds: readonly TId[],
  currentId: TId | null | undefined,
): FlowAnchor<TId> | null {
  return anchorForCandidates(loadedIds, [currentId]);
}

export function resolveModeSwitchFlowAnchor<TId extends FlowItemId>(
  loadedIds: readonly TId[],
  selectedId: TId | null | undefined,
  outgoingId: TId | null | undefined,
): FlowAnchor<TId> | null {
  return anchorForCandidates(loadedIds, [selectedId, outgoingId]);
}

export function resolveQueryResetFlowAnchor<TId extends FlowItemId>(
  loadedIds: readonly TId[],
): FlowAnchor<TId> | null {
  return anchorForCandidates(loadedIds, []);
}

export function estimateFlowItemSize(input: EstimateFlowItemSizeInput): number {
  const width = positiveNumber(input.availableWidth, 1);
  const aspectRatio = positiveNumber(input.aspectRatio, 16 / 9);
  const minHeight = positiveNumber(input.minHeight, 1);
  const requestedMax = positiveNumber(input.maxHeight, Number.POSITIVE_INFINITY);
  const maxHeight = Math.max(minHeight, requestedMax);
  return Math.min(maxHeight, Math.max(minHeight, width / aspectRatio));
}

export function flowPageStep(input: FlowPageStepInput): number {
  if (
    !Number.isFinite(input.viewportSize)
    || input.viewportSize <= 0
    || !Number.isFinite(input.itemSize)
    || input.itemSize <= 0
  ) {
    return 1;
  }
  const gap = Number.isFinite(input.gap) ? Math.max(0, input.gap ?? 0) : 0;
  return Math.max(1, Math.floor((input.viewportSize + gap) / (input.itemSize + gap)));
}

export function visibleFlowThumbnailRange(
  input: VisibleFlowThumbnailRangeInput,
): FlowIndexRange | null {
  const itemCount = normalizeItemCount(input.itemCount);
  if (
    itemCount === 0
    || !Number.isFinite(input.viewportSize)
    || input.viewportSize <= 0
    || !Number.isFinite(input.itemSize)
    || input.itemSize <= 0
  ) {
    return null;
  }

  const scrollOffset = Number.isFinite(input.scrollOffset)
    ? Math.max(0, input.scrollOffset)
    : 0;
  const gap = Number.isFinite(input.gap) ? Math.max(0, input.gap ?? 0) : 0;
  const stride = input.itemSize + gap;
  const viewportEnd = scrollOffset + input.viewportSize;
  let firstVisible = Math.floor(scrollOffset / stride);
  if (firstVisible * stride + input.itemSize <= scrollOffset) firstVisible += 1;
  const lastVisible = Math.ceil(viewportEnd / stride) - 1;
  if (firstVisible > lastVisible || firstVisible >= itemCount || lastVisible < 0) return null;

  const overscan = positiveInteger(input.overscan, 0);
  return {
    startIndex: Math.max(0, firstVisible - overscan),
    endIndex: Math.min(itemCount - 1, lastVisible + overscan),
  };
}

export function visibleMeasuredFlowThumbnailRange<TId extends FlowItemId>(
  input: VisibleMeasuredFlowThumbnailRangeInput<TId>,
): FlowIndexRange | null {
  const itemCount = normalizeItemCount(input.itemCount);
  if (
    itemCount === 0
    || !Number.isFinite(input.viewportStart)
    || !Number.isFinite(input.viewportSize)
    || input.viewportSize <= 0
  ) {
    return null;
  }

  const viewportEnd = input.viewportStart + input.viewportSize;
  let firstVisible = Number.POSITIVE_INFINITY;
  let lastVisible = Number.NEGATIVE_INFINITY;
  for (const item of input.items) {
    if (
      !Number.isSafeInteger(item.index)
      || item.index < 0
      || item.index >= itemCount
      || !Number.isFinite(item.start)
      || !Number.isFinite(item.size)
      || item.size <= 0
    ) {
      continue;
    }
    const itemEnd = item.start + item.size;
    if (itemEnd > input.viewportStart && item.start < viewportEnd) {
      firstVisible = Math.min(firstVisible, item.index);
      lastVisible = Math.max(lastVisible, item.index);
    }
  }
  if (!Number.isFinite(firstVisible) || !Number.isFinite(lastVisible)) return null;

  const overscan = nonNegativeInteger(input.overscan, 0);
  return {
    startIndex: Math.max(0, firstVisible - overscan),
    endIndex: Math.min(itemCount - 1, lastVisible + overscan),
  };
}

export function shouldRequestFlowNextPage(input: FlowNextPageRequestInput): boolean {
  const itemCount = normalizeItemCount(input.itemCount);
  if (
    itemCount === 0
    || !input.hasMore
    || input.loadingMore
    || input.refreshing
    || input.automaticAppendPaused
    || input.visibleEndIndex == null
    || !Number.isFinite(input.visibleEndIndex)
  ) {
    return false;
  }
  const threshold = nonNegativeInteger(input.threshold, 3);
  return Math.trunc(input.visibleEndIndex) >= Math.max(0, itemCount - 1 - threshold);
}

export function flowScrollBehavior(
  reducedMotion: boolean,
  immediate = false,
): FlowScrollBehavior {
  return reducedMotion || immediate ? 'auto' : 'smooth';
}

export function isEnhancedFlowMediaEligible(
  input: EnhancedFlowMediaEligibility,
): boolean {
  return input.active
    && input.settled
    && input.centered
    && input.selected
    && !input.reducedMotion
    && (input.mediaType === 'image' || input.mediaType === 'gif' || input.mediaType === 'video');
}

export function flowStatePresentation(
  baseClassName: string,
  state: FlowVisualState,
): FlowStatePresentation {
  return {
    className: [
      baseClassName.trim(),
      state.active ? 'is-active' : '',
      state.settled ? 'is-settled' : '',
      state.centered ? 'is-centered' : '',
      state.hovered ? 'is-hovered' : '',
      state.selected ? 'is-selected' : '',
      state.current ? 'is-current' : '',
      state.applying ? 'is-applying' : '',
      state.pending ? 'is-pending' : '',
      state.favorite ? 'is-favorite' : '',
    ].filter(Boolean).join(' '),
    attributes: {
      'data-active': state.active || undefined,
      'data-settled': state.settled || undefined,
      'data-centered': state.centered || undefined,
      'data-hovered': state.hovered || undefined,
      'data-selected': state.selected || undefined,
      'data-current': state.current || undefined,
      'data-applying': state.applying || undefined,
      'data-pending': state.pending || undefined,
      'data-favorite': state.favorite || undefined,
    },
  };
}

export function flowStateLabels(state: FlowStateLabelInput): string[] {
  return [
    state.selected ? 'Selected' : '',
    state.current ? 'Current' : '',
    state.applying ? 'Applying' : '',
    state.pending ? 'Pending' : '',
    state.favorite ? 'Favorite' : '',
  ].filter(Boolean);
}

export function resolveFlowScrollIntent(
  centeredIndex: number | null | undefined,
  itemCount: number,
): FlowCenterIntent | null {
  const index = validLoadedIndex(centeredIndex, itemCount);
  return index == null ? null : { type: 'center', index };
}

export function resolveFlowHoverIntent(
  hoveredIndex: number | null | undefined,
  itemCount: number,
): FlowHoverIntent | null {
  const index = validLoadedIndex(hoveredIndex, itemCount);
  return index == null ? null : { type: 'hover', index };
}

function anchorForCandidates<TId extends FlowItemId>(
  loadedIds: readonly TId[],
  candidates: readonly (TId | null | undefined)[],
): FlowAnchor<TId> | null {
  if (loadedIds.length === 0) return null;
  for (const candidate of candidates) {
    if (candidate == null) continue;
    const index = loadedIds.findIndex((id) => Object.is(id, candidate));
    if (index >= 0) return { id: loadedIds[index], index };
  }
  return { id: loadedIds[0], index: 0 };
}

function normalizeItemCount(itemCount: number): number {
  return Number.isFinite(itemCount) ? Math.max(0, Math.trunc(itemCount)) : 0;
}

function positiveInteger(value: number | undefined, fallback: number): number {
  return value != null && Number.isFinite(value) && value > 0
    ? Math.max(1, Math.trunc(value))
    : fallback;
}

function positiveNumber(value: number | null | undefined, fallback: number): number {
  return value != null && Number.isFinite(value) && value > 0 ? value : fallback;
}

function nonNegativeInteger(value: number | undefined, fallback: number): number {
  return value != null && Number.isFinite(value) && value >= 0
    ? Math.trunc(value)
    : fallback;
}

function validLoadedIndex(
  index: number | null | undefined,
  itemCount: number,
): number | null {
  if (
    index == null
    || !Number.isSafeInteger(index)
    || index < 0
    || index >= normalizeItemCount(itemCount)
  ) {
    return null;
  }
  return index;
}
