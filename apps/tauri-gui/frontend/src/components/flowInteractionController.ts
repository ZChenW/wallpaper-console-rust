import type { FlowAnchor } from './wallpaperFlowModel.ts';

export type WallpaperFlowAnchor = FlowAnchor<number>;
export type FlowMotionKind = 'startup' | 'smooth' | 'navigation' | 'resize' | 'query';
export type FlowInteractionPhase =
  | 'unpositioned'
  | 'settled'
  | 'tracking'
  | 'programmatic'
  | 'resize';

export interface FlowDatasetObservation {
  readonly wallpaperIds: readonly number[];
  readonly currentWallpaperId: number | null;
  readonly currentObservationReady: boolean;
  readonly resetKey: string;
  readonly replaceCount: number;
}

export interface FlowPositionIntent {
  readonly anchor: WallpaperFlowAnchor;
  readonly direct: boolean;
  readonly kind: FlowMotionKind;
}

export interface FlowInteractionSnapshot {
  readonly phase: FlowInteractionPhase;
  readonly committedAnchor: WallpaperFlowAnchor | null;
  readonly trackingCandidate: WallpaperFlowAnchor | null;
  readonly programmaticTarget: WallpaperFlowAnchor | null;
  readonly resizeAnchorId: number | null;
  readonly settled: boolean;
  readonly userInteracted: boolean;
}

export interface FlowInteractionInitialState {
  readonly initialAnchor: WallpaperFlowAnchor | null;
  readonly directStartup: boolean;
  readonly currentObservationReady: boolean;
  readonly resetKey: string;
  readonly replaceCount: number;
  readonly queryReplacementPending: boolean;
}

function anchorAt(
  wallpaperIds: readonly number[],
  wallpaperId: number | null,
  fallbackIndex = 0,
): WallpaperFlowAnchor | null {
  if (wallpaperIds.length === 0) return null;
  const requestedIndex = wallpaperId === null ? -1 : wallpaperIds.indexOf(wallpaperId);
  const index = requestedIndex >= 0
    ? requestedIndex
    : Math.min(Math.max(0, fallbackIndex), wallpaperIds.length - 1);
  return { id: wallpaperIds[index]!, index };
}

export class FlowInteractionController {
  private readonly initial: FlowInteractionInitialState;
  private phase: FlowInteractionPhase = 'unpositioned';
  private committedAnchor: WallpaperFlowAnchor | null = null;
  private trackingCandidate: WallpaperFlowAnchor | null;
  private programmaticTarget: WallpaperFlowAnchor | null = null;
  private resizeAnchorId: number | null = null;
  private userInteracted = false;
  private initialized = false;
  private startupAnchorResolved: boolean;
  private previousResetKey: string;
  private previousReplaceCount: number;
  private pendingQueryReset: { resetKey: string; replaceCount: number } | null;
  private appendRequestKey: string | null = null;

  constructor(initial: FlowInteractionInitialState) {
    this.initial = initial;
    this.trackingCandidate = initial.initialAnchor;
    this.startupAnchorResolved = !initial.directStartup || initial.currentObservationReady;
    this.previousResetKey = initial.resetKey;
    this.previousReplaceCount = initial.replaceCount;
    this.pendingQueryReset = initial.queryReplacementPending
      ? { resetKey: initial.resetKey, replaceCount: initial.replaceCount }
      : null;
  }

  snapshot(): FlowInteractionSnapshot {
    return {
      phase: this.phase,
      committedAnchor: this.committedAnchor,
      trackingCandidate: this.trackingCandidate,
      programmaticTarget: this.programmaticTarget,
      resizeAnchorId: this.resizeAnchorId,
      settled: this.phase === 'settled',
      userInteracted: this.userInteracted,
    };
  }

  activeIndex(fallback = 0): number {
    return this.programmaticTarget?.index
      ?? this.trackingCandidate?.index
      ?? this.committedAnchor?.index
      ?? fallback;
  }

  noteUserIntent(): void {
    this.userInteracted = true;
    this.startupAnchorResolved = true;
  }

  beginDirectInput(): void {
    const alreadyTracking = this.phase === 'tracking';
    this.noteUserIntent();
    this.programmaticTarget = null;
    this.resizeAnchorId = null;
    if (!alreadyTracking) {
      this.trackingCandidate = this.committedAnchor ?? this.trackingCandidate;
    }
    this.phase = 'tracking';
  }

  beginProgrammatic(anchor: WallpaperFlowAnchor, kind: FlowMotionKind): void {
    this.programmaticTarget = anchor;
    this.trackingCandidate = anchor;
    this.resizeAnchorId = kind === 'resize' ? anchor.id : null;
    this.phase = kind === 'resize' ? 'resize' : 'programmatic';
  }

  cancelProgrammatic(): void {
    this.programmaticTarget = null;
    this.resizeAnchorId = null;
    this.phase = 'tracking';
  }

  abortAdapterMotion(): void {
    this.programmaticTarget = null;
    this.resizeAnchorId = null;
    if (this.committedAnchor === null && !this.userInteracted) {
      this.initialized = false;
      this.trackingCandidate = this.initial.initialAnchor;
      this.phase = 'unpositioned';
      return;
    }
    this.phase = 'tracking';
  }

  trackCandidate(anchor: WallpaperFlowAnchor): boolean {
    if (this.phase === 'programmatic' || this.phase === 'resize') return false;
    this.trackingCandidate = anchor;
    this.phase = 'tracking';
    return true;
  }

  finishProgrammatic(anchor: WallpaperFlowAnchor): boolean {
    const target = this.programmaticTarget;
    if (target === null || target.id !== anchor.id || target.index !== anchor.index) {
      return false;
    }
    this.committedAnchor = anchor;
    this.trackingCandidate = anchor;
    this.programmaticTarget = null;
    this.resizeAnchorId = null;
    this.phase = 'settled';
    return true;
  }

  noteResize(): number | null {
    this.resizeAnchorId = this.programmaticTarget?.id
      ?? this.committedAnchor?.id
      ?? this.trackingCandidate?.id
      ?? null;
    return this.resizeAnchorId;
  }

  cancelResize(): void {
    this.resizeAnchorId = null;
    if (this.phase === 'resize') this.phase = 'tracking';
  }

  claimAppend(requestKey: string): boolean {
    if (this.appendRequestKey === requestKey) return false;
    this.appendRequestKey = requestKey;
    return true;
  }

  releaseAppend(): void {
    this.appendRequestKey = null;
  }

  observeDataset(input: FlowDatasetObservation): FlowPositionIntent | null {
    const resetChanged = this.previousResetKey !== input.resetKey;
    const replacementChanged = this.previousReplaceCount !== input.replaceCount;
    this.previousResetKey = input.resetKey;
    this.previousReplaceCount = input.replaceCount;

    if (resetChanged) {
      this.startupAnchorResolved = true;
      if (replacementChanged) {
        this.pendingQueryReset = null;
        const anchor = anchorAt(input.wallpaperIds, null);
        this.initialized = anchor !== null;
        return anchor ? { anchor, direct: true, kind: 'query' } : null;
      }
      this.pendingQueryReset = {
        resetKey: input.resetKey,
        replaceCount: input.replaceCount,
      };
      return null;
    }

    if (
      this.pendingQueryReset?.resetKey === input.resetKey
      && this.pendingQueryReset.replaceCount !== input.replaceCount
    ) {
      this.pendingQueryReset = null;
      const anchor = anchorAt(input.wallpaperIds, null);
      this.initialized = anchor !== null;
      return anchor ? { anchor, direct: true, kind: 'query' } : null;
    }
    if (this.pendingQueryReset?.resetKey === input.resetKey && this.initialized) return null;

    if (input.wallpaperIds.length === 0) return null;
    if (!this.initialized) {
      this.initialized = true;
      const initialAnchor = anchorAt(
        input.wallpaperIds,
        this.initial.initialAnchor?.id ?? null,
        this.initial.initialAnchor?.index ?? 0,
      );
      return initialAnchor ? { anchor: initialAnchor, direct: true, kind: 'startup' } : null;
    }

    if (
      this.initial.directStartup
      && !this.startupAnchorResolved
      && input.currentObservationReady
    ) {
      this.startupAnchorResolved = true;
      const current = anchorAt(input.wallpaperIds, input.currentWallpaperId);
      if (!this.userInteracted && input.currentWallpaperId !== null
        && current?.id === input.currentWallpaperId) {
        return { anchor: current, direct: false, kind: 'startup' };
      }
    }

    const stable = this.committedAnchor ?? this.trackingCandidate;
    const stableIndex = stable === null ? -1 : input.wallpaperIds.indexOf(stable.id);
    if (stableIndex >= 0) {
      if (stableIndex === stable!.index) return null;
      return {
        anchor: { id: stable!.id, index: stableIndex },
        direct: false,
        kind: 'smooth',
      };
    }
    const fallback = anchorAt(input.wallpaperIds, null, stable?.index ?? 0);
    return fallback ? { anchor: fallback, direct: false, kind: 'smooth' } : null;
  }
}
