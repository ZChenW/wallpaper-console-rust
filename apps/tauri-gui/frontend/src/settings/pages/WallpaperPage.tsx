import { getSettingsByCategoryAndLevel } from '../configSchema';
import type { WallpaperPageProps } from '../types';
import SettingsPageShell from '../components/SettingsPageShell';
import PageSection from '../components/PageSection';
import ConfigRow from '../components/ConfigRow';

export default function WallpaperPage({ configs, saving, onSet }: WallpaperPageProps) {
  const regular = getSettingsByCategoryAndLevel('wallpaper', false);
  const advanced = getSettingsByCategoryAndLevel('wallpaper', true);

  return (
    <SettingsPageShell
      title="Wallpaper"
      description="Configure image, GIF, and video wallpaper backends."
    >
      <PageSection
        title="Backends"
        description="Images and GIFs use awww for smooth transitions. Videos use mpvpaper with audio enabled and crop-fill by default."
      >
        {regular.map((c) => (
          <ConfigRow key={c.key} setting={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => onSet(c.key, v)} />
        ))}
      </PageSection>

      {advanced.length > 0 && (
        <details className="settings-advanced">
          <summary>Advanced</summary>
          {advanced.map((c) => (
            <ConfigRow key={c.key} setting={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => onSet(c.key, v)} />
          ))}
        </details>
      )}
    </SettingsPageShell>
  );
}
