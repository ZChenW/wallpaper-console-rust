export default function InfoCallout({ children, tone = 'info' }: { children: React.ReactNode; tone?: 'info' | 'warning' | 'danger' }) {
  return <div className={`settings-callout settings-callout-tone-${tone}`}>{children}</div>;
}
