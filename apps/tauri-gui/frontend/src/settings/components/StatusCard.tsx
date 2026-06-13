import type { StatusCardProps } from '../types';

export default function StatusCard({ label, value, detail }: StatusCardProps) {
  return (
    <div className="status-card">
      <span className="status-card-label">{label}</span>
      <span className="status-card-value">{value}</span>
      {detail && <span className="status-card-detail">{detail}</span>}
    </div>
  );
}
