type PageSectionVariant = 'card' | 'plain';

export default function PageSection({
  title,
  description,
  children,
  variant = 'card',
}: {
  title?: string;
  description?: string;
  children: React.ReactNode;
  variant?: PageSectionVariant;
}) {
  return (
    <section className={`setting-section setting-section-${variant}`}>
      {title && <h3 className="section-title">{title}</h3>}
      {description && <p className="section-description">{description}</p>}
      {variant === 'card' ? <div className="section-card">{children}</div> : children}
    </section>
  );
}
