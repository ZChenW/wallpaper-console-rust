interface Props {
  title: string;
  message: string;
  onConfirm: () => void;
  onCancel: () => void;
  danger?: boolean;
}

export default function ConfirmDialog({ title, message, onConfirm, onCancel, danger }: Props) {
  return (
    <div className="dialog-overlay">
      <div className="dialog">
        <h3 className="dialog-title">{title}</h3>
        <p className="dialog-message">{message}</p>
        <div className="dialog-actions">
          <button onClick={onCancel}>Cancel</button>
          <button className={danger ? 'danger' : 'primary'} onClick={onConfirm}>
            Confirm
          </button>
        </div>
      </div>
    </div>
  );
}
