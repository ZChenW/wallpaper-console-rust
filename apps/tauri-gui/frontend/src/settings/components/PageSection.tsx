export default function PageSection({ title, children }: { title?: string; children: React.ReactNode }) {
  return (
    <section className="settings-group">
      {title && <h3>{title}</h3>}
      {children}
    </section>
  );
}
