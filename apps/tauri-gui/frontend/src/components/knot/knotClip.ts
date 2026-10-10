/**
 * Plays a wallpaper's preview clip over its assembled picture.
 *
 * WebKitGTK cannot play a <video> from Tauri's asset protocol (the media engine rejects the scheme),
 * so the backend makes a small silent clip, it is read whole and played from a blob: URL. The clip is
 * an ordinary element laid over the canvas, not a texture: uploading every frame to WebGL measured
 * about 7.5 ms a frame, while the element costs only its decoding. Its frames cannot be read back here
 * either (a 2D canvas, an ImageBitmap and a texture all come out black), so the pieces cannot carry the
 * frame that was playing: when the picture leaves, the clip follows it and fades out over the still.
 */
export const CLIP_DWELL_MS = 400;
export const CLIP_FADE_MS = 220;
// Short: the pieces are apart within about five frames, and a lingering clip reads as a ghost of the whole picture.
export const CLIP_FADE_OUT_MS = 90;
export const CLIP_CACHE_SIZE = 3;
// A clip started while the page is busy (most of all while the app is still loading) often shows
// its first frame and then never advances: `paused` is false, `readyState` is 4, `currentTime` stays
// at 0. Measured: three launches in four on the test machine, one in three on the real desktop.
// Pausing, playing again and seeking do not free it; loading the element again and playing once it
// reports `canplay` did every time, so that is both how a clip is started and how it is recovered.
// The clip stays transparent until its time has actually moved, so a stuck first frame and its
// reload are never seen. (It must not be `visibility: hidden` or zero-sized meanwhile: WebKit does
// not advance a video it considers out of sight.)
export const CLIP_WATCH_MS = 450;
export const CLIP_POLL_MS = 50;
export const CLIP_RECOVERIES = 2;
export const CLIP_READY_TIMEOUT_MS = 8000;

export interface ClipRect { readonly left: number; readonly top: number; readonly width: number; readonly height: number }
export interface ClipTarget {
  /** Identifies the wallpaper to the renderer. */
  readonly key: string;
  readonly path: string;
  /** Where its assembled picture is right now, in CSS pixels relative to the video's offset parent. */
  readonly place: () => ClipRect | null;
}
export type ClipState = 'idle' | 'loading' | 'playing';
export interface ClipHost {
  readonly load: (path: string) => Promise<ArrayBuffer | null>;
  /** Told when a clip starts being fetched (its first generation can take seconds), plays, or stops. */
  readonly onState?: (state: ClipState) => void;
  readonly createUrl?: (bytes: ArrayBuffer) => string;
  readonly revokeUrl?: (url: string) => void;
  readonly schedule?: (callback: () => void, ms: number) => unknown;
  readonly cancel?: (handle: unknown) => void;
  readonly frame?: (callback: (now: number) => void) => void;
}

export class KnotClipPlayer {
  private readonly urls = new Map<string, string>();
  private wanted: ClipTarget | null = null;
  private loaded: { key: string; url: string } | null = null;
  private shown = false;
  /** The picture a fading-out clip is still following. */
  private leaving: ClipTarget | null = null;
  private opacity = 0;
  private running = true;
  private state: ClipState = 'idle';
  private ramp = 0;
  private request = 0;
  private dwell: unknown = null;
  private disposed = false;
  private readonly createUrl: (bytes: ArrayBuffer) => string;
  private readonly revokeUrl: (url: string) => void;
  private readonly schedule: (callback: () => void, ms: number) => unknown;
  private readonly cancel: (handle: unknown) => void;
  private readonly frame: (callback: (now: number) => void) => void;

  constructor(private readonly video: HTMLVideoElement, private readonly host: ClipHost) {
    this.createUrl = host.createUrl ?? ((bytes) => URL.createObjectURL(new Blob([bytes], { type: 'video/mp4' })));
    this.revokeUrl = host.revokeUrl ?? ((url) => URL.revokeObjectURL(url));
    this.schedule = host.schedule ?? ((callback, ms) => setTimeout(callback, ms));
    this.cancel = host.cancel ?? ((handle) => clearTimeout(handle as ReturnType<typeof setTimeout>));
    this.frame = host.frame ?? ((callback) => { requestAnimationFrame(callback); });
  }

  private setState(state: ClipState): void {
    if (this.state === state) return;
    this.state = state;
    this.host.onState?.(state);
  }

  get playingKey() { return this.shown ? this.wanted?.key ?? null : null; }

  /** The wallpaper whose clip should be playing, or null. Calling it again with the same key is free. */
  want(target: ClipTarget | null): void {
    if (this.disposed || (target?.key ?? null) === (this.wanted?.key ?? null)) { if (target) this.wanted = target; return; }
    // Straight back to the picture whose clip is still fading out: it never stopped, so bring it back.
    if (target && this.leaving?.key === target.key && !this.video.paused) {
      this.request += 1; this.leaving = null; this.wanted = target; this.shown = true;
      this.video.dataset.visible = 'true';
      this.setState('playing');
      this.fade(1, CLIP_FADE_MS);
      return;
    }
    this.stop(true);
    this.wanted = target;
    if (!target) return;
    const request = ++this.request;
    // Passing over a video must not start decoding it, let alone generating its clip.
    this.dwell = this.schedule(() => { this.dwell = null; void this.start(target, request); }, CLIP_DWELL_MS);
  }

  /** Keep the clip on its picture: after a resize, while the rope swings, and while it fades out on the move. */
  reposition(): void {
    const target = this.shown ? this.wanted : this.leaving;
    if (target) this.place(target);
  }

  /**
   * Freeze the clip where it is, or let it run again. For a window that is not being used: a frozen
   * frame looks like the still it replaced and costs nothing, where a playing clip costs a tenth of a
   * core for as long as the view is open.
   */
  setRunning(running: boolean): void {
    if (this.disposed || this.running === running) return;
    this.running = running;
    if (!this.shown) return;
    if (running) void this.video.play().catch(() => undefined); else this.video.pause();
  }

  /** Stop at once: for a pause or a teardown, not for a picture about to move. */
  halt(): void { this.stop(false); this.wanted = null; }

  dispose(): void {
    if (this.disposed) return;
    this.halt();
    this.disposed = true;
    this.video.removeAttribute('src'); this.video.load?.();
    for (const url of this.urls.values()) this.revokeUrl(url);
    this.urls.clear(); this.loaded = null;
  }

  private place(target: ClipTarget): boolean {
    const rect = target.place();
    if (!rect || rect.width <= 0 || rect.height <= 0) return false;
    const style = this.video.style;
    style.left = `${rect.left}px`; style.top = `${rect.top}px`; style.width = `${rect.width}px`; style.height = `${rect.height}px`;
    return true;
  }

  private async start(target: ClipTarget, request: number): Promise<void> {
    const current = () => !this.disposed && request === this.request && this.wanted?.key === target.key;
    this.setState('loading');
    const done = await this.begin(target, request, current);
    if (!done && current()) this.setState('idle');
  }

  /** Resolves true once the clip is showing. */
  private async begin(target: ClipTarget, request: number, current: () => boolean): Promise<boolean> {
    let url = this.urls.get(target.path);
    if (url) { this.urls.delete(target.path); this.urls.set(target.path, url); } else {
      const bytes = await this.host.load(target.path).catch(() => null);
      if (!bytes || this.disposed) return false;
      // Keep it even if the camera has moved on: coming back should not generate or read it again.
      url = this.createUrl(bytes);
      this.urls.set(target.path, url);
      for (const [path, old] of this.urls) {
        if (this.urls.size <= CLIP_CACHE_SIZE) break;
        if (old === this.loaded?.url || old === url) continue;
        this.urls.delete(path); this.revokeUrl(old);
      }
    }
    if (!current()) return false;
    // The same clip is still loaded when the camera returns to a picture it just left: it resumes on
    // the very frame the pieces are showing.
    // A new clip must not replace one that is still fading out on another picture.
    if (this.leaving) this.finishLeaving();
    if (!this.place(target)) return false;
    if (this.loaded?.key !== target.key || this.loaded.url !== url) {
      this.video.src = url; this.loaded = { key: target.key, url };
      if (!await this.ready() || !current()) return false;
    }
    for (let attempt = 0; attempt <= CLIP_RECOVERIES; attempt++) {
      try { await this.video.play(); } catch { return false; }
      if (!current()) { this.video.pause(); return false; }
      if (await this.advancing(current)) {
        if (!current()) return false;
        this.reveal(request);
        if (!this.running) this.video.pause();
        return true;
      }
      if (!current()) return false;
      if (attempt === CLIP_RECOVERIES) break;
      this.video.load();
      if (!await this.ready() || !current()) return false;
    }
    // It never got going: leave the still, and stop decoding.
    this.video.pause();
    return false;
  }

  /** Whether playback really moves within CLIP_WATCH_MS (a stuck clip reports playing and stays at one time). */
  private advancing(current: () => boolean): Promise<boolean> {
    const from = this.video.currentTime;
    return new Promise((resolve) => {
      let waited = 0;
      const look = () => {
        if (!current()) { resolve(false); return; }
        if (this.video.currentTime > from + 0.02) { resolve(true); return; }
        waited += CLIP_POLL_MS;
        if (waited >= CLIP_WATCH_MS) resolve(false); else this.schedule(look, CLIP_POLL_MS);
      };
      this.schedule(look, CLIP_POLL_MS);
    });
  }

  /** Resolves once the element can play what it was given (false on an error or after too long). */
  private ready(): Promise<boolean> {
    if (this.video.readyState >= 3) return Promise.resolve(true);
    return new Promise((resolve) => {
      let timer: unknown = null;
      const finish = (ok: boolean) => {
        this.video.removeEventListener('canplay', yes); this.video.removeEventListener('error', no);
        if (timer !== null) this.cancel(timer);
        resolve(ok);
      };
      const yes = () => finish(true), no = () => finish(false);
      this.video.addEventListener('canplay', yes); this.video.addEventListener('error', no);
      timer = this.schedule(() => { timer = null; finish(false); }, CLIP_READY_TIMEOUT_MS);
    });
  }

  /** Opacity is driven from requestAnimationFrame: a compositor-run transition flashes its start when it ends here. */
  private fade(to: 0 | 1, ms: number, done?: () => void): void {
    const ramp = ++this.ramp, from = this.opacity;
    let start: number | null = null;
    const step = (now: number) => {
      if (this.disposed || ramp !== this.ramp) return;
      start ??= now;
      const progress = Math.min(1, (now - start) / (ms * Math.abs(to - from) || 1));
      this.opacity = from + (to - from) * progress;
      this.video.style.opacity = String(this.opacity);
      if (progress < 1) this.frame(step); else done?.();
    };
    this.frame(step);
  }

  private reveal(request: number): void {
    if (request !== this.request) return;
    this.shown = true;
    this.place(this.wanted!);
    this.video.dataset.visible = 'true';
    this.opacity = 0; this.video.style.opacity = '0';
    this.setState('playing');
    this.fade(1, CLIP_FADE_MS);
  }

  private finishLeaving(): void {
    this.ramp += 1; this.leaving = null;
    this.opacity = 0; this.video.style.opacity = '0';
    this.video.pause();
    delete this.video.dataset.visible;
  }

  private stop(fadeOut: boolean): void {
    this.request += 1;
    this.setState('idle');
    if (this.dwell !== null) { this.cancel(this.dwell); this.dwell = null; }
    if (this.shown && fadeOut && this.wanted) {
      // Keeps playing and following its picture while it fades into the still underneath.
      this.shown = false; this.leaving = this.wanted;
      this.fade(0, CLIP_FADE_OUT_MS, () => this.finishLeaving());
      return;
    }
    const active = this.shown || this.wanted !== null || this.leaving !== null;
    this.shown = false;
    if (active) this.finishLeaving();
  }
}
