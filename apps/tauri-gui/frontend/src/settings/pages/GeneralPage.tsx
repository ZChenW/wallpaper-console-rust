import type { GeneralPageProps } from '../types';
import SettingsPageShell from '../components/SettingsPageShell';
import PageSection from '../components/PageSection';
import StatusCard from '../components/StatusCard';
import ConfigRow from '../components/ConfigRow';
import { getSettingsByCategoryAndLevel } from '../configSchema';

export default function GeneralPage({
  libraryStatus,
  weStatus,
  thumbCache,
  configs,
  saving,
  onSet,
}: GeneralPageProps) {
  const preferenceSettings = getSettingsByCategoryAndLevel('general', false);

  return (
    <SettingsPageShell
      title="General"
      description="Review app status and core runtime health."
    >
      <PageSection title="Preferences">
        {preferenceSettings.map((setting) => (
          <ConfigRow
            key={setting.key}
            setting={setting}
            value={configs[setting.key] ?? ''}
            saving={saving === setting.key}
            onSet={(value) => onSet(setting.key, value)}
          />
        ))}
      </PageSection>
      <PageSection title="Status">
        <StatusCard
          label="Database"
          value={libraryStatus != null ? `${libraryStatus.sqliteRows} wallpapers indexed` : '...'}
        />
        <StatusCard
          label="Wallpaper Engine Scene"
          value={weStatus?.available ? `Ready — ${weStatus.path}` : 'Missing'}
          detail={!weStatus?.available ? (weStatus?.message ?? 'Checking...') : undefined}
          tone={weStatus?.available ? 'success' : 'warning'}
        />
        <StatusCard
          label="Thumbnail Cache"
          value={thumbCache
            ? `${thumbCache.entries} thumbnails, ${thumbCache.size}${thumbCache.failureEntries > 0 ? ` · ${thumbCache.failureEntries} failed` : ''}`
            : '...'}
        />
      </PageSection>
    </SettingsPageShell>
  );
}
