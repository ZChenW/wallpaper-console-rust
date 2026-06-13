import { getSettingsByCategoryAndLevel } from '../configSchema';
import type { WallpaperPageProps } from '../types';
import PageSection from '../components/PageSection';
import ConfigRow from '../components/ConfigRow';

export default function WallpaperPage({ configs, saving, onSet }: WallpaperPageProps) {
  const regular = getSettingsByCategoryAndLevel('wallpaper', false);
  const advanced = getSettingsByCategoryAndLevel('wallpaper', true);

  return (
    <div className="settings-page">
      <PageSection title="Backends">
        <p className="config-desc">Images and GIFs are recommended with <strong>awww</strong>; videos with <strong>mpvpaper</strong>.</p>
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
    </div>
  );
}
