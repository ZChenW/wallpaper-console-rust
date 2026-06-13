import { api } from '../../api/bridge';
import { commandErrorFeedback, commandSuccessFeedback } from '../../api/feedback';
import { getSettingsByCategoryAndLevel, cleanupDays } from '../configSchema';
import type { LibraryPageProps } from '../types';
import PageSection from '../components/PageSection';
import StatusCard from '../components/StatusCard';
import ConfigRow from '../components/ConfigRow';

export default function LibraryPage({
  configs,
  saving,
  onSet,
  thumbCache,
  onFeedback,
  handleCleanupThumbnails,
  loadThumbCache,
  confirmAndRun,
  operationLock,
}: LibraryPageProps) {
  const regular = getSettingsByCategoryAndLevel('library', false);
  const advanced = getSettingsByCategoryAndLevel('library', true);

  return (
    <div className="settings-page">
      <PageSection title="Scan Filters">
        {regular.map((c) => (
          <ConfigRow key={c.key} setting={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => onSet(c.key, v)} />
        ))}
      </PageSection>

      <PageSection title="Thumbnail Cache">
        <StatusCard
          label="Status"
          value={thumbCache
            ? `${thumbCache.entries} thumbnails, ${thumbCache.size}${thumbCache.failureEntries > 0 ? ` · ${thumbCache.failureEntries} failed` : ''}`
            : '...'}
          detail={thumbCache
            ? `Cleanup: older than ${cleanupDays(configs, thumbCache)} days`
            : undefined}
        />
        <div className="db-actions">
          <button
            className="btn small"
            onClick={handleCleanupThumbnails}
            disabled={operationLock}
            title={`Remove cached thumbnails older than ${cleanupDays(configs, thumbCache)} days`}
          >
            Cleanup old
          </button>
          <button
            className="btn small danger"
            disabled={operationLock}
            onClick={() => confirmAndRun(
              'Clear Thumbnail Cache',
              `Delete all ${thumbCache?.entries ?? 0} cached thumbnails?`,
              async () => {
                onFeedback({ state: 'running', label: 'Clearing thumbnail cache' });
                try {
                  const r = await api.thumbnailCacheClear();
                  if (r.success) {
                    onFeedback(commandSuccessFeedback('Thumbnail cache clear', r));
                  } else {
                    onFeedback(commandErrorFeedback('Thumbnail cache clear', r));
                  }
                  loadThumbCache();
                } catch (e) {
                  onFeedback(commandErrorFeedback('Thumbnail cache clear', e));
                }
              },
              true,
            )}
          >
            Clear all
          </button>
        </div>
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
