import { getSettingsByCategoryAndLevel } from '../configSchema';
import type { AdvancedPageProps } from '../types';
import PageSection from '../components/PageSection';
import ConfigRow from '../components/ConfigRow';

function maskHome(value: string): string {
  return value.replace(/\/home\/[^/\s]+/g, '~');
}

export default function AdvancedPage({
  configs,
  saving,
  onSet,
  weDebugInfo,
  showRawConfig,
  setShowRawConfig,
}: AdvancedPageProps) {
  const advanced = getSettingsByCategoryAndLevel('advanced', false);

  return (
    <div className="settings-page">
      <PageSection title="Development">
        {advanced.map((c) => (
          <ConfigRow key={c.key} setting={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => onSet(c.key, v)} />
        ))}
      </PageSection>

      <PageSection title="Known Settings">
        <p className="config-desc" style={{ marginBottom: 8 }}>
          Read-only view of recognised configuration key/value pairs.
        </p>
        <label className="settings-toggle">
          <input
            type="checkbox"
            checked={showRawConfig}
            onChange={(e) => setShowRawConfig(e.target.checked)}
          />
          <span>Show known config keys</span>
        </label>
        {showRawConfig && (
          <div className="settings-raw-config">
            {Object.entries(configs)
              .filter(([, v]) => v !== '')
              .sort(([a], [b]) => a.localeCompare(b))
              .map(([key, value]) => (
                <div key={key} className="raw-row">
                  <code className="raw-key">{key}</code>
                  <code className="raw-value">{maskHome(value)}</code>
                </div>
              ))}
          </div>
        )}
      </PageSection>

      {weDebugInfo && (weDebugInfo.lastStderr || weDebugInfo.lastExitStatus) && (
        <PageSection title="WE Backend Debug Info">
          <p className="config-desc" style={{ fontSize: '0.8em', color: 'var(--muted)', marginBottom: 8 }}>
            Shows command lines, log paths, and stderr. Home directory paths are shortened to ~, but other local paths may remain visible. Avoid sharing screenshots.
          </p>
          <div className="debug-block">
            {weDebugInfo.lastCommandLine && (
              <div className="debug-row">
                <span className="debug-label">Last command:</span>
                <code className="debug-value">{maskHome(weDebugInfo.lastCommandLine)}</code>
              </div>
            )}
            {weDebugInfo.lastTargetConfig && (
              <div className="debug-row">
                <span className="debug-label">Target config:</span>
                <code className="debug-value">{maskHome(weDebugInfo.lastTargetConfig)}</code>
              </div>
            )}
            {weDebugInfo.lastExitStatus && (
              <div className="debug-row">
                <span className="debug-label">Exit status:</span>
                <code className="debug-value">{maskHome(weDebugInfo.lastExitStatus)}</code>
              </div>
            )}
            {weDebugInfo.lastStderr && (
              <details className="debug-row">
                <summary className="debug-label" style={{ cursor: 'pointer' }}>
                  Last stderr (click to expand)
                </summary>
                <pre className="debug-value debug-pre">{maskHome(weDebugInfo.lastStderr)}</pre>
              </details>
            )}
            <div className="debug-row">
              <span className="debug-label">Log file:</span>
              <code className="debug-value">{maskHome(weDebugInfo.logPath)}</code>
            </div>
          </div>
        </PageSection>
      )}
    </div>
  );
}
