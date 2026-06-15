import { useEffect, useRef } from 'react';
import { ContextAction } from './WallpaperGrid';
import { emitFeedback } from '../events/appEvents';

interface Props {
  x: number;
  y: number;
  path: string;
  actions: ContextAction[];
  onClose: () => void;
}

export default function ContextMenu({ x, y, path, actions, onClose }: Props) {
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
      {actions.map((a) => (
        <button
          key={a.label}
          className={a.danger ? 'danger' : ''}
          onClick={async () => {
            try {
              await a.action(path);
            } catch (e) {
              emitFeedback({ state: 'error', label: a.label, detail: String(e) });
            } finally {
              onClose();
            }
          }}
        >
          {a.label}
        </button>
      ))}
    </div>
  );
}
