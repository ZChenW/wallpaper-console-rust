/**
 * Advance Web Animations from the main thread instead of letting them play.
 *
 * WebKitGTK runs a playing transform/opacity animation on the compositor. When it ends, the
 * compositor drops it a frame or two before the main thread commits the end state, and for those
 * frames the layer shows the values it was last given: the first keyframe. On screen that is a
 * flash of the starting picture at the end of every such animation (seen in a screen recording of
 * the real app; it never appears in DOM measurements or WebDriver screenshots).
 *
 * The keyframes, easing and fill stay in the Web Animations engine. Each animation is held
 * paused and its clock is set from requestAnimationFrame, so every frame's values, including the
 * last, are committed by the main thread. `lead.onfinish` is called once the clock reaches the
 * end, as it would have been; owners keep cancelling the animations exactly as before, and a
 * cancelled set stops being driven.
 */
export function driveAnimations(animations: readonly Animation[], lead: Animation | undefined = animations[0]): void {
  const driven = animations.filter((animation) => typeof animation.pause === 'function' && animation.effect);
  if (driven.length === 0 || typeof requestAnimationFrame !== 'function') return;
  const duration = Math.max(...driven.map((animation) => Number(animation.effect?.getComputedTiming().endTime ?? 0)));
  if (!(duration > 0)) return;
  for (const animation of driven) {
    animation.pause();
    animation.currentTime = 0;
  }
  let start: number | null = null;
  const tick = (now: number) => {
    if (driven.some((animation) => animation.playState === 'idle')) return;
    start ??= now;
    const time = Math.min(duration, now - start);
    for (const animation of driven) animation.currentTime = time;
    if (time < duration) {
      requestAnimationFrame(tick);
      return;
    }
    const handler = lead?.onfinish;
    if (lead && handler) handler.call(lead, new Event('finish') as AnimationPlaybackEvent);
  };
  requestAnimationFrame(tick);
}
