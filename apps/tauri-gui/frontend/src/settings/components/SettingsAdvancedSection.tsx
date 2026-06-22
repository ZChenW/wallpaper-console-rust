import type { ReactNode } from 'react';

interface SettingsAdvancedSectionProps {
  children: ReactNode;
  onCollapse?: () => void;
}

export default function SettingsAdvancedSection({
  children,
  onCollapse,
}: SettingsAdvancedSectionProps) {
  return (
    <details
      className="settings-advanced"
      onToggle={(event) => {
        if (!event.currentTarget.open) onCollapse?.();
      }}
    >
      <summary>Advanced</summary>
      {children}
    </details>
  );
}
