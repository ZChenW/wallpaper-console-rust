import { Loader } from 'lucide-react';
import { api } from '../../api/bridge';
import { commandErrorFeedback, commandSuccessFeedback } from '../../api/feedback';
import type { DatabasePageProps } from '../types';
import SettingsPageShell from '../components/SettingsPageShell';
import PageSection from '../components/PageSection';
import StatusCard from '../components/StatusCard';
import { resolveDatabaseStatusCard } from '../statusCards';

export default function DatabasePage({
  libraryStatus,
  libraryStatusError,
  libraryStatusLoading,
  dbAction,
  operationLock,
  runDbAction,
  onFeedback,
  confirmAndRun,
  onRestore,
  restoreInputRef,
  onRestoreFileSelected,
  invalidateLibrary,
  refreshSettingsStatus,
  diagnosticsRunning,
  runDiagnosticsExport,
}: DatabasePageProps) {
  const busy = dbAction !== null || operationLock;
  const databaseCard = resolveDatabaseStatusCard(libraryStatus, libraryStatusError, libraryStatusLoading);

  return (
    <SettingsPageShell
      title="Database"
      description="Verify, back up, rebuild, and export the library database."
    >
      <PageSection title="Status">
        <StatusCard
          label="SQLite active"
          value={databaseCard.value}
          detail={databaseCard.detail}
          tone={databaseCard.tone}
        />
      </PageSection>

      <PageSection title="Maintenance" variant="plain">
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
            className={`btn small danger ${dbAction === 'rebuild' ? 'running' : ''}`}
            onClick={() => confirmAndRun(
              'Rebuild Database',
              'Re-scan all configured source directories and rebuild the library database?',
              async () => {
                try {
                  const r = await api.rescan();
                  if (r.success) {
                    invalidateLibrary();
                    onFeedback(commandSuccessFeedback('Rebuild', r));
                  } else {
                    onFeedback(commandErrorFeedback('Rebuild', r));
                  }
                } finally {
                  refreshSettingsStatus('db-rebuild');
                }
              },
              true,
              'rebuild',
            )}
            disabled={busy}
          >
            {dbAction === 'rebuild' && <Loader size={12} className="spin" style={{ marginRight: 4 }} />}
            Rebuild Database
          </button>
          <button
            className="btn small danger"
            onClick={onRestore}
            disabled={busy}
          >
            Restore Backup
          </button>
          <input
            ref={restoreInputRef}
            type="file"
            accept=".db,.db.bak*"
            style={{ display: 'none' }}
            onChange={onRestoreFileSelected}
          />
        </div>
      </PageSection>

      <PageSection title="Compatibility / Export" variant="plain">
        <div className="db-actions">
          <button
            className={`btn small ${dbAction === 'export' ? 'running' : ''}`}
            onClick={() => confirmAndRun(
              'Export Legacy Files',
              'Export SQLite back to flat files?',
              async () => {
                onFeedback({ state: 'running', label: 'Export' });
                try {
                  const r = await api.sqliteExportFlat();
                  if (r.success) {
                    onFeedback(commandSuccessFeedback('Export', r));
                  } else {
                    onFeedback(commandErrorFeedback('Export', r));
                  }
                } finally {
                  refreshSettingsStatus('db-export');
                }
              },
              true,
              'export',
            )}
            disabled={busy}
          >
            {dbAction === 'export' && <Loader size={12} className="spin" style={{ marginRight: 4 }} />}
            Export Legacy Files
          </button>
          <button
            className={`btn small ${diagnosticsRunning ? 'running' : ''}`}
            disabled={busy || diagnosticsRunning}
            onClick={runDiagnosticsExport}
          >
            {diagnosticsRunning && <Loader size={12} className="spin" style={{ marginRight: 4 }} />}
            Export diagnostics
          </button>
        </div>
      </PageSection>
    </SettingsPageShell>
  );
}
