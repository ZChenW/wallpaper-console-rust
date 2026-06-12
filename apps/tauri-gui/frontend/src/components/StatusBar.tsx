import { Monitor, HardDrive, Image, Loader } from 'lucide-react';
import { StatusDTO } from '../api/bridge';
import { CommandFeedback } from '../api/feedback';

interface Props {
  status: StatusDTO | null;
  applying: boolean;
  feedback: CommandFeedback;
}

export default function StatusBar({ status, applying, feedback }: Props) {
  return (
    <footer className="statusbar">
      <div className="statusbar-left">
        {applying && feedback.state === 'idle' && <Loader size={14} className="spin" />}
        {feedback.state === 'running' && <Loader size={14} className="spin" />}
        <Monitor size={14} />
        <span className="statusbar-text">
          {feedback.state === 'running' && feedback.label}
          {feedback.state !== 'running' && (status?.current ? status.current.split('/').pop() : '(none)')}
        </span>
        {feedback.state === 'idle' && status?.lastBackend && status.lastBackend !== '(none)' && (
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