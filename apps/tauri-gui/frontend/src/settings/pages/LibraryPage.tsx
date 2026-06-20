import { api } from '../../api/bridge';
import { commandErrorFeedback, commandSuccessFeedback } from '../../api/feedback';
import { getSettingsByCategoryAndLevel, cleanupDays } from '../configSchema';
import type { LibraryPageProps } from '../types';
import SettingsPageShell from '../components/SettingsPageShell';
import PageSection from '../components/PageSection';
import StatusCard from '../components/StatusCard';
import ConfigRow from '../components/ConfigRow';
import { resolveThumbnailStatusCard } from '../statusCards';

export default function LibraryPage({
  configs,
  saving,
  onSet,
  thumbCache,
  thumbCacheError,
  thumbCacheLoading,
  onFeedback,
  handleCleanupThumbnails,
  refreshSettingsStatus,
  confirmAndRun,
  operationLock,
}: LibraryPageProps) {
  const regular = getSettingsByCategoryAndLevel('library', false);
  const advanced = getSettingsByCategoryAndLevel('library', true);
  const thumbCard = resolveThumbnailStatusCard(
    thumbCache,
    thumbCacheError,
    thumbCacheLoading,
    { cleanupDays: cleanupDays(configs, thumbCache) },
  );

  return (
    <SettingsPageShell
      title="Library"
      description="Control scan filters, thumbnails, and cache cleanup."
    >
      <PageSection
        title="Scan Filters"
        description="These filters apply during library scan and database rebuild."
      >
        {regular.map((c) => (
          <ConfigRow key={c.key} setting={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => onSet(c.key, v)} />
        ))}
      </PageSection>

      <PageSection
        title="Thumbnail Cache"
        description="Cached thumbnails make large libraries faster to browse."
      >
        <StatusCard
          label="Status"
          value={thumbCard.value}
          detail={thumbCard.detail}
          tone={thumbCard.tone}
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
                } catch (e) {
                  onFeedback(commandErrorFeedback('Thumbnail cache clear', e));
                } finally {
                  refreshSettingsStatus('thumbnail-clear');
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
    </SettingsPageShell>
  );
}
