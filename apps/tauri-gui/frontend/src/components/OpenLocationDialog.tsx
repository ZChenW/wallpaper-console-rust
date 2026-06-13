import { useEffect, useRef } from 'react';

interface Props {
  path: string;
  onSelect: (mode: 'files' | 'terminal') => void;
  onClose: () => void;
}

export default function OpenLocationDialog({ path, onSelect, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [onClose]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [onClose]);

  return (
    <div className="dialog-overlay">
      <div className="dialog" ref={ref}>
        <h3 className="dialog-title">Open Project Folder</h3>
        <p className="dialog-message">{path}</p>
        <div className="dialog-actions">
          <button className="primary" onClick={() => onSelect('files')}>Open in Files</button>
          <button onClick={() => onSelect('terminal')}>Open in Terminal</button>
        </div>
      </div>
    </div>
  );
}
