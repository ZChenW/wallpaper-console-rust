import type { GeneralPageProps } from '../types';
import PageSection from '../components/PageSection';
import StatusCard from '../components/StatusCard';

export default function GeneralPage({
  libraryStatus,
  weStatus,
  thumbCache,
}: GeneralPageProps) {
  return (
    <div className="settings-page">
      <PageSection title="Status">
        <StatusCard
          label="Database"
          value={libraryStatus != null ? `${libraryStatus.sqliteRows} wallpapers indexed` : '...'}
        />
        <StatusCard
          label="Wallpaper Engine Scene"
          value={weStatus?.available ? `Ready — ${weStatus.path}` : 'Missing'}
          detail={!weStatus?.available ? (weStatus?.message ?? 'Checking...') : undefined}
        />
        <StatusCard
          label="Thumbnail Cache"
          value={thumbCache
            ? `${thumbCache.entries} thumbnails, ${thumbCache.size}${thumbCache.failureEntries > 0 ? ` · ${thumbCache.failureEntries} failed` : ''}`
            : '...'}
        />
      </PageSection>
    </div>
  );
}
