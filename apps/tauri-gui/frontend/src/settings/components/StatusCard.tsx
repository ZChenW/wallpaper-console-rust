import type { StatusCardProps } from '../types';

export default function StatusCard({ label, value, detail, tone = 'neutral' }: StatusCardProps) {
  return (
    <div className={`status-card status-card-tone-${tone}`}>
      <span className="status-card-label">{label}</span>
      <span className="status-card-value">{value}</span>
      {detail && <span className="status-card-detail">{detail}</span>}
    </div>
  );
}
