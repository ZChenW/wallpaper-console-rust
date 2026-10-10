import type { LibraryViewMode } from './shellPreferences.ts';
import { driveAnimations } from '../components/drivenAnimation.ts';

// Book owns its session state and transition; Knot only borrows the layout.
export const libraryChromeHidden = (mode: LibraryViewMode, bookImmersive: boolean) =>
  mode === 'knot' || (mode === 'book' && bookImmersive);

/** A mode/layout commit precedes this fade. Never play on WebKit's compositor. */
export function fadeLibraryModeStage(stage: HTMLElement, reducedMotion: boolean) {
  if (reducedMotion) return () => undefined;
  const animation = stage.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 150, easing: 'ease-out', fill: 'both' });
  animation.onfinish = () => animation.cancel();
  driveAnimations([animation]);
  return () => { animation.onfinish = null; animation.cancel(); };
}

export type BookImmersivePhase = 'out' | 'in' | 'return';

/** The only layout commit is between these phases, while all changing content
 * is transparent. Enter fades chrome in the OLD layout; leave restores it in
 * the NEW layout. A phase-one interruption returns without a layout switch;
 * a phase-two interruption queues the opposite transition until arrival.
 */
export function bookImmersivePhasePlan(entering: boolean, reducedMotion = false) {
  return {
    out: { duration: reducedMotion ? 0 : 110, easing: 'ease-in',
      content: [{ opacity: 1, transform: 'scale(1)' }, { opacity: 0, transform: 'scale(0.985)' }],
      chrome: entering ? [{ opacity: 1 }, { opacity: 0 }] : null },
    in: { duration: reducedMotion ? 0 : 190, easing: 'ease-out',
      content: [{ opacity: 0, transform: 'scale(1.015)' }, { opacity: 1, transform: 'scale(1)' }],
      chrome: entering ? null : [{ opacity: 0 }, { opacity: 1 }] },
    interrupt: { out: 'return-without-switch', in: 'finish-then-opposite' } as const,
  };
}

export function bookImmersiveInterruption(phase: BookImmersivePhase, current: boolean, target: boolean, requested: boolean) {
  if (phase === 'in') return { action: 'queue' as const, target: requested };
  // F and the action button derive their toggle from the committed layout.
  // Before the switch, a second identical request means return to that layout.
  const next = phase === 'out' && requested === target ? current : requested;
  return { action: next === current ? 'return' as const : 'out' as const, target: next };
}

const chromeSelector = '.single-page-topbar, .single-page-library-controls, .single-page-statusbar';

/** Imperative adapter keeps sampled animation frames out of React. Only commit
 * updates React, synchronously, so the child's scale repaint precedes phase two.
 */
export function createBookImmersiveTransition(shell: HTMLElement, commit: (value: boolean, synchronous?: boolean) => void, focus: () => void) {
  let current = false;
  let target = false;
  let queued = false;
  let phase: BookImmersivePhase | null = null;
  let animations: Animation[] = [];
  let content: HTMLElement[] = [];
  let chrome: HTMLElement[] = [];
  const cancel = () => {
    animations.forEach((animation) => { animation.onfinish = null; animation.cancel(); });
    animations = [];
  };
  const sample = () => [...content, ...chrome].map((element) => {
    const css = getComputedStyle(element);
    return { element, opacity: css.opacity, transform: css.transform };
  });
  const animate = (nextPhase: BookImmersivePhase, sampled?: ReturnType<typeof sample>) => {
    const plan = bookImmersivePhasePlan(target);
    const step = nextPhase === 'out' ? plan.out : plan.in;
    // Prepare new animations before canceling the filled old set. They override
    // it in the same frame, including at the hidden switch and on interruption.
    const old = animations;
    phase = nextPhase;
    const end = step.content[1];
    animations = content.map((element) => {
      const start = sampled?.find((value) => value.element === element);
      return element.animate([start ? { opacity: start.opacity, transform: start.transform } : step.content[0], end],
        { duration: step.duration, easing: step.easing, fill: 'both' });
    });
    const chromeFrames = nextPhase === 'return' && !current ? [{ opacity: 0 }, { opacity: 1 }] : step.chrome;
    if (chromeFrames) chrome.forEach((element) => {
      const start = sampled?.find((value) => value.element === element);
      animations.push(element.animate([start ? { opacity: start.opacity } : chromeFrames[0], chromeFrames[1]],
        { duration: step.duration, easing: step.easing, fill: 'both' }));
    });
    old.forEach((animation) => { animation.onfinish = null; animation.cancel(); });
    const arrival = animations[0];
    if (!arrival) { phase = null; commit(target); current = target; return; }
    arrival.onfinish = () => {
      if (phase === 'out') {
        // Remove the phase-one scale while fully transparent so layout effects
        // measure the new leaves without inheriting an animated ancestor scale.
        const holds = content.map((element) => element.animate(
          [{ opacity: 0, transform: 'scale(1)' }], { duration: 0, fill: 'both' },
        ));
        animations.push(...holds);
        current = target;
        phase = 'in'; // A synchronous automatic exit during commit must queue.
        commit(current);
        focus();
        setOrigins();
        animate('in');
      } else {
        phase = null;
        cancel();
        if (queued !== current) request(queued, false);
      }
    };
    driveAnimations([...animations], arrival);
  };
  const setOrigins = () => {
    const stage = shell.querySelector<HTMLElement>('.wallpaper-book__stage');
    const stageRect = stage?.getBoundingClientRect();
    if (stageRect) content.forEach((element) => {
      const rect = element.getBoundingClientRect();
      element.style.transformOrigin = `${stageRect.left + stageRect.width / 2 - rect.left}px ${stageRect.top + stageRect.height / 2 - rect.top}px`;
    });
  };
  const request = (value: boolean, reducedMotion: boolean) => {
    if (reducedMotion || !shell.querySelector('.wallpaper-book')) {
      cancel();
      phase = null;
      current = target = queued = value;
      commit(value, false);
      focus();
      return;
    }
    if (phase) {
      const interruption = bookImmersiveInterruption(phase, current, target, value);
      queued = interruption.target;
      if (interruption.action === 'queue') return;
      const sampled = sample();
      target = interruption.target;
      animate(interruption.action === 'return' ? 'return' : 'out', sampled);
      return;
    }
    if (value === current) return;
    target = queued = value;
    content = [...shell.querySelectorAll<HTMLElement>('.wallpaper-book')];
    const zoom = document.querySelector<HTMLElement>('.wallpaper-book__zoom-content');
    if (zoom) content.push(zoom);
    setOrigins();
    chrome = [...shell.querySelectorAll<HTMLElement>(chromeSelector)];
    animate('out');
  };
  return { request, dispose: cancel };
}
