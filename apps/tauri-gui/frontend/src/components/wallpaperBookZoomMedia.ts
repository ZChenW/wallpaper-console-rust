/** Decode off-screen, then commit one src assignment with a final lifecycle
 * guard. A close/input can invalidate the request even between decode and commit.
 */
export async function decodeBookStill(
  source: string,
  commit: (image: HTMLImageElement) => void,
  current: () => boolean,
  createImage: () => HTMLImageElement = () => new Image(),
): Promise<void> {
  const image = createImage();
  image.src = source;
  try {
    await image.decode();
    if (current()) commit(image);
  } catch {
    // Keep the already-painted picture on authorization/decoding failure.
  }
}

/** loadeddata alone can be a poster/paused frame. Wait for playback AND an
 * actual presented frame; the fallback also requires HAVE_CURRENT_DATA.
 */
export function watchBookVideoReady(video: HTMLVideoElement, ready: () => void): () => void {
  let disposed = false;
  let frame: number | null = null;
  const notify = () => {
    frame = null;
    if (!disposed && !video.paused && video.readyState >= 2) ready();
  };
  const playing = () => {
    if (disposed || video.paused || video.readyState < 2 || frame !== null) return;
    if (typeof video.requestVideoFrameCallback === 'function') frame = video.requestVideoFrameCallback(notify);
    else notify();
  };
  video.addEventListener('playing', playing);
  video.addEventListener('loadeddata', playing);
  playing();
  return () => {
    disposed = true;
    video.removeEventListener('playing', playing);
    video.removeEventListener('loadeddata', playing);
    if (frame !== null) video.cancelVideoFrameCallback(frame);
  };
}

/** Hold the real page on this frame until the landing commit. A video page has
 * no loaded img/currentSrc; using its poster would change the first/last picture.
 */
export function captureBookVideoStill(
  video: HTMLVideoElement,
  createCanvas: () => HTMLCanvasElement = () => document.createElement('canvas'),
): { source: string; resume: () => void } | null {
  // WebKitGTK clip frames read back as black. The page's already decoded large still is
  // the zoom/turn handoff, and its clip player owns pause/resume independently of this capture.
  if (video.dataset?.previewClip || video.src?.startsWith('blob:')) return null;
  if (video.readyState < 2 || video.videoWidth <= 0 || video.videoHeight <= 0) return null;
  const wasPlaying = !video.paused;
  video.pause();
  const resume = () => {
    if (wasPlaying && video.isConnected) void video.play().catch(() => {});
  };
  try {
    const canvas = createCanvas();
    canvas.width = video.videoWidth;
    canvas.height = video.videoHeight;
    const context = canvas.getContext('2d');
    if (!context) { resume(); return null; }
    context.drawImage(video, 0, 0);
    return { source: canvas.toDataURL(), resume };
  } catch {
    resume();
    return null;
  }
}

/** Detached outgoing zoom copies carry the existing still, never a cloned clip decoder/frame. */
export function prepareBookClipStillHandoff(picture: HTMLElement, outgoing: HTMLElement): boolean {
  if (!picture.querySelector('video[data-preview-clip]')) return false;
  outgoing.querySelector('.wallpaper-book__zoom-media')?.remove();
  const still = outgoing.querySelector<HTMLElement>('.wallpaper-book__zoom-still');
  if (still) still.style.opacity = '1';
  return true;
}
