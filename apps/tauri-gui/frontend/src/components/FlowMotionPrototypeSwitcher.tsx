import { useEffect } from 'react';

import {
  FLOW_MOTION_PROTOTYPE_VARIANTS,
  type FlowMotionPrototypeVariant,
} from './flowMotionPrototypeVariant.ts';

interface FlowMotionPrototypeSwitcherProps {
  readonly value: FlowMotionPrototypeVariant;
  readonly onChange: (variant: FlowMotionPrototypeVariant) => void;
}

export default function FlowMotionPrototypeSwitcher({
  value,
  onChange,
}: FlowMotionPrototypeSwitcherProps) {
  const index = FLOW_MOTION_PROTOTYPE_VARIANTS.indexOf(value);
  const move = (direction: -1 | 1) => {
    onChange(FLOW_MOTION_PROTOTYPE_VARIANTS[
      (index + direction + FLOW_MOTION_PROTOTYPE_VARIANTS.length)
        % FLOW_MOTION_PROTOTYPE_VARIANTS.length
    ]!);
  };

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      if (
        target instanceof HTMLInputElement
        || target instanceof HTMLTextAreaElement
        || (target instanceof HTMLElement && target.isContentEditable)
      ) {
        return;
      }
      if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
      event.preventDefault();
      move(event.key === 'ArrowLeft' ? -1 : 1);
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [index, onChange]);

  const label = value === 'balanced'
    ? 'B — Balanced'
    : 'C — Distance-driven';

  return (
    <nav
      aria-label="Flow motion prototype variants"
      className="flow-motion-prototype-switcher"
    >
      <button aria-label="Previous Flow motion variant" onClick={() => move(-1)} type="button">
        ←
      </button>
      <span>
        <small>PROTOTYPE · NOT PRODUCTION</small>
        {label}
      </span>
      <button aria-label="Next Flow motion variant" onClick={() => move(1)} type="button">
        →
      </button>
    </nav>
  );
}
