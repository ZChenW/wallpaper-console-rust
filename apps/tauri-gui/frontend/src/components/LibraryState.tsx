import type { ReactNode } from 'react';

interface LibraryStateProps {
  readonly icon: ReactNode;
  readonly title: string;
  readonly description: ReactNode;
  readonly action?: ReactNode;
  readonly children?: ReactNode;
  readonly className?: string;
  readonly role?: 'alert' | 'status';
}

export default function LibraryState({
  icon,
  title,
  description,
  action,
  children,
  className,
  role,
}: LibraryStateProps) {
  return (
    <section
      className={['single-page-empty', 'library-state', className].filter(Boolean).join(' ')}
      role={role}
    >
      <span aria-hidden="true" className="library-state__icon">{icon}</span>
      <h2>{title}</h2>
      <p className="library-state__description">{description}</p>
      {action ? <div className="single-page-empty__actions">{action}</div> : null}
      {children}
    </section>
  );
}
