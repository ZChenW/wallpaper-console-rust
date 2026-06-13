import { useEffect, useRef } from 'react';
import { ContextAction } from './WallpaperGrid';

interface Props {
  x: number;
  y: number;
  path: string;
  canApply: boolean;
  onApply: (path: string) => void;
  actions: ContextAction[];
  onClose: () => void;
}

export default function ContextMenu({ x, y, path, canApply, onApply, actions, onClose }: Props) {
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
      {canApply && (
        <button onClick={async () => {
          try { await onApply(path); } catch (e) { /* apply failures handled in App */ }
          onClose();
        }}>Apply</button>
      )}
      {actions.map((a) => (
        <button
          key={a.label}
          className={a.danger ? 'danger' : ''}
          onClick={async () => {
            try {
              await a.action(path);
            } catch (e) {
              window.dispatchEvent(new CustomEvent('wc-feedback', {
                detail: { state: 'error', label: a.label, detail: String(e) },
              }));
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
