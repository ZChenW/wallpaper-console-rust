import type { GeneralPageProps } from '../types';
import SettingsPageShell from '../components/SettingsPageShell';
import PageSection from '../components/PageSection';
import StatusCard from '../components/StatusCard';
import ConfigRow from '../components/ConfigRow';
import { getSettingsByCategoryAndLevel } from '../configSchema';
import {
  resolveDatabaseStatusCard,
  resolveThumbnailStatusCard,
  resolveWeStatusCard,
} from '../statusCards';

export default function GeneralPage({
  libraryStatus,
  libraryStatusError,
  libraryStatusLoading,
  weStatus,
  weStatusError,
  weStatusLoading,
  thumbCache,
  thumbCacheError,
  thumbCacheLoading,
  configs,
  saving,
  onSet,
}: GeneralPageProps) {
  const preferenceSettings = getSettingsByCategoryAndLevel('general', false);
  const databaseCard = resolveDatabaseStatusCard(libraryStatus, libraryStatusError, libraryStatusLoading);
  const weCard = resolveWeStatusCard(weStatus, weStatusError, weStatusLoading);
  const thumbCard = resolveThumbnailStatusCard(thumbCache, thumbCacheError, thumbCacheLoading);

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
        <StatusCard label="Database" value={databaseCard.value} detail={databaseCard.detail} tone={databaseCard.tone} />
        <StatusCard label="Wallpaper Engine Scene" value={weCard.value} detail={weCard.detail} tone={weCard.tone} />
        <StatusCard label="Thumbnail Cache" value={thumbCard.value} detail={thumbCard.detail} tone={thumbCard.tone} />
      </PageSection>
    </SettingsPageShell>
  );
}
