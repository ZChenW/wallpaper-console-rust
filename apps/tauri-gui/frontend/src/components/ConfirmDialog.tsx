import { Loader } from 'lucide-react';

interface Props {
  title: string;
  message: string;
  onConfirm: () => void;
  onCancel: () => void;
  danger?: boolean;
  confirming?: boolean;
  confirmLabel?: string;
}

export default function ConfirmDialog({ title, message, onConfirm, onCancel, danger, confirming, confirmLabel }: Props) {
  return (
    <div className="dialog-overlay">
      <div className="dialog">
        <h3 className="dialog-title">{title}</h3>
        <p className="dialog-message">{message}</p>
        <div className="dialog-actions">
          <button onClick={onCancel} disabled={confirming}>Cancel</button>
          <button
            className={danger ? 'danger' : 'primary'}
            onClick={onConfirm}
            disabled={confirming}
          >
            {confirming && <Loader size={12} className="spin" style={{ marginRight: 6 }} />}
            {confirming ? (confirmLabel ?? 'Running...') : (confirmLabel ?? 'Confirm')}
          </button>
        </div>
      </div>
    </div>
  );
}