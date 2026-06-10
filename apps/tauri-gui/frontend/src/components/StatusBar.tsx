import { Monitor, HardDrive, Image, Loader } from 'lucide-react';
import { StatusDTO } from '../api/bridge';

interface Props {
  status: StatusDTO | null;
  applying: boolean;
}

export default function StatusBar({ status, applying }: Props) {
  return (
    <footer className="statusbar">
      <div className="statusbar-left">
        {applying && <Loader size={14} className="spin" />}
        <Monitor size={14} />
        <span className="statusbar-text">
          {status?.current
            ? status.current.split('/').pop()
            : '(none)'}
        </span>
        {status?.lastBackend && status.lastBackend !== '(none)' && (
          <span className="statusbar-badge">{status.lastBackend}</span>
        )}
      </div>
      <div className="statusbar-right">
        <HardDrive size={14} />
        <span className="statusbar-text">{status?.configDir ?? '...'}</span>
        <span className="statusbar-sep">|</span>
        <Image size={14} />
        <span className="statusbar-text">{status?.sourceCount ?? 0} sources</span>
      </div>
    </footer>
  );
}
