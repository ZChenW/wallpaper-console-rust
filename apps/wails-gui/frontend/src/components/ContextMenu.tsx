import { useEffect, useRef } from 'react';
import { ContextAction } from './WallpaperGrid';

interface Props {
  x: number;
  y: number;
  path: string;
  onApply: (path: string) => void;
  actions: ContextAction[];
  onClose: () => void;
}

export default function ContextMenu({ x, y, path, onApply, actions, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [onClose]);

  return (
    <div className="context-menu" ref={ref} style={{ left: x, top: y }}>
      <button onClick={() => { onApply(path); onClose(); }}>Apply</button>
      {actions.map((a) => (
        <button
          key={a.label}
          className={a.danger ? 'danger' : ''}
          onClick={() => { a.action(path); onClose(); }}
        >
          {a.label}
        </button>
      ))}
    </div>
  );
}
