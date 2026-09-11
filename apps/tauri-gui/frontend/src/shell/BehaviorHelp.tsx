import { useLayoutEffect, useRef, useState } from 'react';

export default function BehaviorHelp({ id, label, text }: {
  readonly id: string;
  readonly label: string;
  readonly text: string;
}) {
  const [open, setOpen] = useState(false);
  const tooltipRef = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    if (!open) return;
    const tooltip = tooltipRef.current;
    if (!tooltip) return;
    const place = () => {
      const panel = tooltip.closest('.settings-panel')?.getBoundingClientRect();
      const left = Math.max(0, panel?.left ?? 0) + 12;
      const right = Math.min(window.innerWidth, panel?.right ?? window.innerWidth) - 24;
      tooltip.style.maxWidth = `min(22rem, ${Math.max(1, right - left)}px)`;
      tooltip.style.marginLeft = '0px';
      const bounds = tooltip.getBoundingClientRect();
      tooltip.style.marginLeft = `${Math.max(left - bounds.left, Math.min(0, right - bounds.right))}px`;
    };
    place();
    window.addEventListener('resize', place);
    return () => window.removeEventListener('resize', place);
  }, [open, text]);
  return (
    <span className="settings-behavior-help-wrap">
      <button
        aria-describedby={id}
        aria-expanded={open}
        aria-controls={id}
        aria-label={label}
        className="settings-behavior-help"
        data-behavior-help={label}
        onClick={() => setOpen((current) => !current)}
        onBlur={() => setOpen(false)}
        onFocus={(event) => { if (event.currentTarget.matches(':focus-visible')) setOpen(true); }}
        onPointerEnter={(event) => { if (event.pointerType === 'mouse') setOpen(true); }}
        onPointerLeave={(event) => { if (event.pointerType === 'mouse') setOpen(false); }}
        onKeyDown={(event) => {
          if (event.key !== 'Escape' || !open) return;
          event.preventDefault();
          event.stopPropagation();
          setOpen(false);
        }}
        type="button"
      >
        <span aria-hidden="true">?</span>
      </button>
      <span className="settings-behavior-tooltip" ref={tooltipRef} id={id} role="tooltip">{text}</span>
    </span>
  );
}
