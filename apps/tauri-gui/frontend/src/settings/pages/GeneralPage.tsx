import type { GeneralPageProps } from '../types';
import SettingsPageShell from '../components/SettingsPageShell';
import PageSection from '../components/PageSection';
import StatusCard from '../components/StatusCard';

export default function GeneralPage({
  libraryStatus,
  weStatus,
  thumbCache,
}: GeneralPageProps) {
  return (
    <SettingsPageShell
      title="General"
      description="Review app status and core runtime health."
    >
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
