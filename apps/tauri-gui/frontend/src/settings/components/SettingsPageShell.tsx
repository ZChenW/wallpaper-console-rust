type Props = {
  title: string;
  description?: string;
  children: React.ReactNode;
};

export default function SettingsPageShell({ title, description, children }: Props) {
  return (
    <div className="settings-page">
      <header className="settings-page-header">
        <h3>{title}</h3>
        <p>{description ?? '\u00a0'}</p>
      </header>
      <div className="settings-page-body">
        {children}
      </div>
    </div>
  );
}
