import { Loader } from 'lucide-react';
import { api } from '../../api/bridge';
import { cleanupDays } from '../configSchema';
import type { GeneralPageProps } from '../types';
import PageSection from '../components/PageSection';
import InfoCallout from '../components/InfoCallout';
import StatusCard from '../components/StatusCard';

export default function GeneralPage({
  libraryStatus,
  weStatus,
  thumbCache,
  configs,
  dbAction,
  operationLock,
  runDbAction,
  handleCleanupThumbnails,
}: GeneralPageProps) {
  const busy = dbAction !== null || operationLock;

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

      <PageSection title="Quick Actions">
        <div className="db-actions">
          <button
            className={`btn small ${dbAction === 'verify' ? 'running' : ''}`}
            onClick={() => runDbAction('verify', 'Verify', () => api.sqliteVerify())}
            disabled={busy}
          >
            {dbAction === 'verify' && <Loader size={12} className="spin" style={{ marginRight: 4 }} />}
            Verify Database
          </button>
          <button
            className={`btn small ${dbAction === 'backup' ? 'running' : ''}`}
            onClick={() => runDbAction('backup', 'Backup', () => api.sqliteBackup())}
            disabled={busy}
          >
            {dbAction === 'backup' && <Loader size={12} className="spin" style={{ marginRight: 4 }} />}
            Backup Database
          </button>
          <button
            className="btn small"
            onClick={handleCleanupThumbnails}
            disabled={busy}
            title={`Remove cached thumbnails older than ${cleanupDays(configs, thumbCache)} days`}
          >
            Cleanup old thumbnails
          </button>
        </div>
      </PageSection>

      <InfoCallout>
        Wallpaper Engine <strong>Web</strong> projects are indexed but unsupported as live wallpapers.
      </InfoCallout>
    </div>
  );
}
