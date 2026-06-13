import { getSettingsByCategoryAndLevel } from '../configSchema';
import type { WallpaperEnginePageProps } from '../types';
import PageSection from '../components/PageSection';
import InfoCallout from '../components/InfoCallout';
import StatusCard from '../components/StatusCard';
import ConfigRow from '../components/ConfigRow';

export default function WallpaperEnginePage({ weStatus, configs, saving, onSet }: WallpaperEnginePageProps) {
  const regular = getSettingsByCategoryAndLevel('we', false);
  const advanced = getSettingsByCategoryAndLevel('we', true);

  return (
    <div className="settings-page">
      <PageSection title="Scene Backend">
        <StatusCard
          label="Status"
          value={weStatus?.available ? 'Ready' : 'Missing'}
          detail={weStatus?.message ?? 'Checking...'}
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

      <InfoCallout>
        Wallpaper Engine <strong>Web</strong> projects appear in Library for metadata and preview only. They are not supported as live wallpapers.
      </InfoCallout>
    </div>
  );
}
