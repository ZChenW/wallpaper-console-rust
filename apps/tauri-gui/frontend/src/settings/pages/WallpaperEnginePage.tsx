import { getSettingsByCategoryAndLevel } from '../configSchema';
import type { WallpaperEnginePageProps } from '../types';
import SettingsPageShell from '../components/SettingsPageShell';
import PageSection from '../components/PageSection';
import InfoCallout from '../components/InfoCallout';
import StatusCard from '../components/StatusCard';
import ConfigRow from '../components/ConfigRow';

export default function WallpaperEnginePage({ weStatus, configs, saving, onSet }: WallpaperEnginePageProps) {
  const regular = getSettingsByCategoryAndLevel('we', false);
  const advanced = getSettingsByCategoryAndLevel('we', true);

  return (
    <SettingsPageShell
      title="Wallpaper Engine"
      description="Configure Wallpaper Engine scene support."
    >
      <PageSection title="Scene Backend">
        <StatusCard
          label="Status"
          value={weStatus?.available ? 'Ready' : 'Missing'}
          detail={weStatus?.message ?? 'Checking...'}
          tone={weStatus?.available ? 'success' : 'warning'}
        />
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

      <InfoCallout tone="warning">
        Wallpaper Engine <strong>Web</strong> projects are indexed for metadata only and cannot be applied as live wallpapers.
      </InfoCallout>
    </SettingsPageShell>
  );
}
