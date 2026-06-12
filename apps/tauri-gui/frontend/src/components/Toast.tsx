import { CheckCircle, AlertTriangle, AlertCircle, X } from 'lucide-react';
import { CommandFeedback } from '../api/feedback';

interface Props {
  feedback: CommandFeedback;
  onDismiss: () => void;
}

export default function Toast({ feedback, onDismiss }: Props) {
  if (feedback.state !== 'success' && feedback.state !== 'error' && feedback.state !== 'warning') return null;

  return (
    <div className={`toast ${feedback.state}`}>
      {feedback.state === 'success' && <CheckCircle size={16} />}
      {feedback.state === 'warning' && <AlertCircle size={16} />}
      {feedback.state === 'error' && <AlertTriangle size={16} />}
      <span className="toast-text">
        {feedback.detail ? `${feedback.label}: ${feedback.detail}` : feedback.label}
      </span>
      <button className="toast-dismiss" onClick={onDismiss}>✕</button>
    </div>
  );
}