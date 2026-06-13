import { Settings, Monitor, MonitorPlay, LibraryBig, Database, Wrench } from 'lucide-react';
import type { SettingsCategory } from './configSchema';
import { CATEGORY_LABELS, CATEGORY_ORDER } from './configSchema';

interface Props {
  active: SettingsCategory;
  onChange: (cat: SettingsCategory) => void;
}

const CATEGORY_ICONS: Record<SettingsCategory, typeof Settings> = {
  general: Settings,
  wallpaper: Monitor,
  we: MonitorPlay,
  library: LibraryBig,
  database: Database,
  advanced: Wrench,
};

export default function SettingsSidebar({ active, onChange }: Props) {
  return (
    <nav className="settings-sidebar" aria-label="Settings categories">
      {CATEGORY_ORDER.map((cat) => {
        const Icon = CATEGORY_ICONS[cat];
        return (
          <button
            key={cat}
            type="button"
            className={`settings-sidebar-btn${active === cat ? ' active' : ''}`}
            aria-current={active === cat ? 'page' : undefined}
            onClick={() => onChange(cat)}
          >
            <Icon size={16} />
            <span className="settings-sidebar-label">{CATEGORY_LABELS[cat]}</span>
          </button>
        );
      })}
    </nav>
  );
}
