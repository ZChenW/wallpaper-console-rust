import { releaseVideoDecoder } from './wallpaperPreviewMedia.ts';

export const PREVIEW_CLIP_CACHE_SIZE = 3;
export const PREVIEW_CLIP_WATCH_MS = 450;
export const PREVIEW_CLIP_POLL_MS = 50;
export const PREVIEW_CLIP_RECOVERIES = 2;
export const PREVIEW_CLIP_READY_TIMEOUT_MS = 8000;
export const PREVIEW_CLIP_FADE_MS = 200;
export const PREVIEW_CLIP_FADE_OUT_MS = 120;

export interface PreviewClipClock {
  schedule(callback: () => void, ms: number): unknown;
  cancel(handle: unknown): void;
  frame(callback: (now: number) => void): unknown;
  cancelFrame(handle: unknown): void;
}

const browserClock: PreviewClipClock = {
  schedule: (callback, ms) => setTimeout(callback, ms),
  cancel: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
  frame: (callback) => requestAnimationFrame(callback),
  cancelFrame: (handle) => cancelAnimationFrame(handle as number),
};

export interface PreviewClipLease {
  readonly ready: Promise<string | null>;
  release(): void;
}

interface CachedClip {
  url: string | null;
  users: number;
  ready: Promise<string | null>;
}

/** Small LRU of preview clips only. Pin before loading, including during a fade or a focus freeze. */
export class PreviewClipUrlCache {
  private readonly entries = new Map<string, CachedClip>();
  private disposed = false;

  constructor(
    private readonly load: (path: string) => Promise<ArrayBuffer | null>,
    private readonly createUrl = (bytes: ArrayBuffer) => URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' })),
    private readonly revokeUrl = (url: string) => URL.revokeObjectURL(url),
    private readonly limit = PREVIEW_CLIP_CACHE_SIZE,
  ) {}

  acquire(path: string): PreviewClipLease {
    if (this.disposed) return { ready: Promise.resolve(null), release() {} };
    let entry = this.entries.get(path);
    if (!entry) {
      const created: CachedClip = { url: null, users: 0, ready: Promise.resolve(null) };
      // Defer the load so the entry and its pin exist even with an immediately resolved loader.
      created.ready = Promise.resolve().then(() => this.load(path)).catch(() => null).then((bytes) => {
        if (this.disposed || !bytes || bytes.byteLength === 0) {
          if (this.entries.get(path) === created) this.entries.delete(path);
          return null;
        }
        try { created.url = this.createUrl(bytes); } catch {
          if (this.entries.get(path) === created) this.entries.delete(path);
          return null;
        }
        this.trim();
        return created.url;
      });
      entry = created;
    }
    entry.users += 1;
    this.entries.delete(path);
    this.entries.set(path, entry);
    this.trim();
    let released = false;
    return {
      ready: entry.ready,
      release: () => {
        if (released) return;
        released = true;
        entry.users -= 1;
        this.trim();
      },
    };
  }

  private trim(): void {
    // Pending reads are not blob URLs yet. Do not evict one: its eventual URL would be orphaned.
    let count = [...this.entries.values()].filter((entry) => entry.url !== null).length;
    for (const [path, entry] of this.entries) {
      if (count <= this.limit) break;
      if (entry.users > 0 || !entry.url) continue;
      this.entries.delete(path);
      this.revokeUrl(entry.url);
      count -= 1;
    }
  }

  /** Owners must detach their decoders before disposing the cache. Pending reads cannot create URLs afterward. */
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const entry of this.entries.values()) if (entry.url) this.revokeUrl(entry.url);
    this.entries.clear();
  }
}

export interface PreviewClipVideo extends EventTarget {
  src: string;
  readonly readyState: number;
  readonly currentTime: number;
  play(): Promise<void>;
  pause(): void;
  load(): void;
}

export interface PreviewClipStart {
  readonly done: Promise<boolean>;
  cancel(): void;
}

/** canplay -> play -> observed time movement. A stuck first frame gets at most two reloads. */
export function startPreviewClip(
  video: PreviewClipVideo,
  source: string,
  clock: PreviewClipClock = browserClock,
): PreviewClipStart {
  const controller = new AbortController();
  const { signal } = controller;
  const ready = (reset: () => void) => new Promise<boolean>((resolve) => {
    let timer: unknown = null;
    const finish = (ok: boolean) => {
      video.removeEventListener('canplay', yes);
      video.removeEventListener('error', no);
      signal.removeEventListener('abort', no);
      if (timer !== null) clock.cancel(timer);
      resolve(ok);
    };
    const yes = () => finish(true), no = () => finish(false);
    video.addEventListener('canplay', yes);
    video.addEventListener('error', no);
    signal.addEventListener('abort', no);
    timer = clock.schedule(no, PREVIEW_CLIP_READY_TIMEOUT_MS);
    reset();
    if (signal.aborted) no();
    else if (video.readyState >= 3) yes();
  });
  const play = () => new Promise<boolean>((resolve) => {
    const finish = (ok: boolean) => { signal.removeEventListener('abort', no); resolve(ok); };
    const no = () => finish(false);
    signal.addEventListener('abort', no);
    void video.play().then(() => finish(!signal.aborted), () => finish(false));
  });
  const advancing = () => new Promise<boolean>((resolve) => {
    const from = video.currentTime;
    let waited = 0;
    let timer: unknown = null;
    const finish = (ok: boolean) => {
      signal.removeEventListener('abort', no);
      if (timer !== null) clock.cancel(timer);
      resolve(ok);
    };
    const no = () => finish(false);
    const look = () => {
      if (signal.aborted) { finish(false); return; }
      if (Math.abs(video.currentTime - from) > 0.02) { finish(true); return; }
      waited += PREVIEW_CLIP_POLL_MS;
      if (waited >= PREVIEW_CLIP_WATCH_MS) finish(false);
      else timer = clock.schedule(look, PREVIEW_CLIP_POLL_MS);
    };
    signal.addEventListener('abort', no);
    timer = clock.schedule(look, PREVIEW_CLIP_POLL_MS);
  });
  const done = (async () => {
    if (!await ready(() => { if (video.src !== source) video.src = source; }) || signal.aborted) {
      if (!signal.aborted) video.pause();
      return false;
    }
    for (let attempt = 0; attempt <= PREVIEW_CLIP_RECOVERIES; attempt += 1) {
      if (signal.aborted || !await play() || signal.aborted) break;
      if (await advancing()) return !signal.aborted;
      if (signal.aborted || attempt === PREVIEW_CLIP_RECOVERIES) break;
      if (!await ready(() => video.load()) || signal.aborted) break;
    }
    // Cancellation already paused synchronously. A stale task must never pause its replacement.
    if (!signal.aborted) video.pause();
    return false;
  })();
  return { done, cancel: () => { controller.abort(); video.pause(); } };
}

export interface PreviewClipPlayerOptions {
  readonly clock?: PreviewClipClock;
  readonly fadeInMs?: number;
  readonly onRunning?: () => void;
  readonly onFailure?: () => void;
}

/** Owns one laid-out, transparent video above its existing still. Eligibility/dwell belong to the view. */
export class PreviewClipPlayer {
  private wanted: string | null = null;
  private loaded: { path: string; lease: PreviewClipLease; url: string | null } | null = null;
  private start: PreviewClipStart | null = null;
  private shown = false;
  private running = false;
  private disposed = false;
  private request = 0;
  private opacity = 0;
  private frame: unknown = null;
  private fadeTarget: 0 | 1 | null = null;
  private readonly clock: PreviewClipClock;
  private readonly mediaError = () => { if (this.shown) this.fail(); };

  constructor(
    private readonly video: HTMLVideoElement,
    private readonly cache: PreviewClipUrlCache,
    private readonly options: PreviewClipPlayerOptions = {},
  ) {
    this.clock = options.clock ?? browserClock;
    video.style.opacity = '0';
    video.addEventListener('error', this.mediaError);
  }

  want(path: string | null): void {
    if (this.disposed || this.wanted === path) return;
    this.wanted = path;
    this.cancelStart();
    if (this.shown && this.loaded?.path === path) {
      this.fade(1);
      if (this.running) this.resume();
      return;
    }
    if (this.shown && path === null) { this.fade(0); return; }
    this.release();
    if (path && this.running) void this.begin(path);
  }

  /** Blur freezes the painted clip in place. No read, decoder start, reload, or fade occurs until focus. */
  setRunning(running: boolean): void {
    if (this.disposed || this.running === running) return;
    this.running = running;
    if (!running) {
      this.cancelStart();
      this.video.pause();
      this.cancelFade();
      return;
    }
    if (this.shown) {
      this.resume();
      if (this.fadeTarget !== null) this.fade(this.fadeTarget);
    } else if (this.wanted) void this.begin(this.wanted);
  }

  private cancelStart(): void {
    this.request += 1;
    this.start?.cancel();
    this.start = null;
  }

  private resume(): void {
    const request = this.request;
    void this.video.play().catch(() => {
      // pause() on a new blur can reject an earlier play promise. That is a freeze, not a failure.
      if (!this.disposed && this.running && request === this.request) this.fail();
    });
  }

  private async begin(path: string): Promise<void> {
    const request = ++this.request;
    const current = () => !this.disposed && this.running && request === this.request && this.wanted === path;
    if (!this.loaded) this.loaded = { path, lease: this.cache.acquire(path), url: null };
    const loaded = this.loaded;
    const url = loaded.url ?? await loaded.lease.ready;
    if (!current()) return;
    if (!url) { this.fail(); return; }
    loaded.url = url;
    this.start = startPreviewClip(this.video, url, this.clock);
    const ok = await this.start.done;
    if (!current()) return;
    this.start = null;
    if (!ok) { this.fail(); return; }
    this.shown = true;
    this.video.dataset.previewRunning = 'true';
    this.fade(1);
    this.options.onRunning?.();
  }

  private fail(): void {
    this.wanted = null;
    this.cancelStart();
    this.release();
    this.options.onFailure?.();
  }

  private cancelFade(): void {
    if (this.frame !== null) this.clock.cancelFrame(this.frame);
    this.frame = null;
  }

  private fade(to: 0 | 1): void {
    this.cancelFade();
    this.fadeTarget = to;
    if (!this.running) return;
    const from = this.opacity;
    const duration = to === 1 ? this.options.fadeInMs ?? PREVIEW_CLIP_FADE_MS : PREVIEW_CLIP_FADE_OUT_MS;
    let start: number | null = null;
    const tick = (now: number) => {
      this.frame = null;
      if (this.disposed || !this.running) return;
      start ??= now;
      const progress = duration === 0 ? 1 : Math.min(1, (now - start) / duration);
      this.opacity = from + (to - from) * progress;
      this.video.style.opacity = String(this.opacity);
      if (progress < 1) this.frame = this.clock.frame(tick);
      else {
        this.fadeTarget = null;
        if (to === 0) this.release();
      }
    };
    this.frame = this.clock.frame(tick);
  }

  private release(): void {
    this.cancelFade();
    this.fadeTarget = null;
    this.shown = false;
    this.opacity = 0;
    this.video.style.opacity = '0';
    delete this.video.dataset.previewRunning;
    releaseVideoDecoder(this.video);
    this.loaded?.lease.release();
    this.loaded = null;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.video.removeEventListener('error', this.mediaError);
    this.cancelStart();
    this.release();
  }
}

/** Listen directly: a parent render/memo must never delay the pause on window blur. */
export function watchPreviewClipFocus(
  player: Pick<PreviewClipPlayer, 'setRunning'>,
  target: EventTarget = window,
  page: Pick<Document, 'hasFocus' | 'hidden' | 'addEventListener' | 'removeEventListener'> = document,
): () => void {
  const focus = () => player.setRunning(page.hasFocus() && !page.hidden);
  const blur = () => player.setRunning(false);
  target.addEventListener('focus', focus);
  target.addEventListener('blur', blur);
  page.addEventListener('visibilitychange', focus);
  focus();
  return () => {
    target.removeEventListener('focus', focus);
    target.removeEventListener('blur', blur);
    page.removeEventListener('visibilitychange', focus);
  };
}
