import React from 'react';
import { getSettingsByCategoryAndLevel } from '../configSchema';
import type { AdvancedPageProps } from '../types';
import SettingsPageShell from '../components/SettingsPageShell';
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
  weDebugError,
  showRawConfig,
  setShowRawConfig,
}: AdvancedPageProps) {
  const advanced = getSettingsByCategoryAndLevel('advanced', false);

  // Show sub-selectors when a mode is explicitly chosen
  const mode = configs['open_project_location_mode'] || 'file_manager';
  const showFileMgr = mode === 'file_manager';
  const showTerminalMgr = mode === 'terminal';
  const isAskMode = configs['open_project_location_mode'] === 'ask';
  const displayOpenMode = mode === 'terminal' ? 'terminal' : 'file_manager';

  // Hide identity rows: don't show the sub-selector as a regular config row
  const identityKeys = new Set(['gui_file_manager', 'gui_file_manager_custom', 'gui_terminal_file_manager', 'gui_terminal_file_manager_custom']);
  const filteredAdvanced = advanced.filter((c) => !identityKeys.has(c.key));

  return (
    <SettingsPageShell
      title="Advanced"
      description="Developer-oriented settings and external tool preferences."
    >
      <PageSection title="Development">
        {filteredAdvanced.map((c) => {
          if (c.key === 'open_project_location_mode') {
            return (
              <React.Fragment key={c.key}>
                <ConfigRow
                  setting={c}
                  value={displayOpenMode}
                  saving={saving === c.key}
                  onSet={(v) => onSet(c.key, v)}
                />
                {isAskMode && (
                  <div className="settings-callout">
                    Current behavior: ask on first use. The next time you open a project folder, you will be prompted to choose between File Manager and Terminal File Manager. Your choice will be saved.
                  </div>
                )}
              </React.Fragment>
            );
          }
          return (
            <ConfigRow key={c.key} setting={c} value={configs[c.key] ?? ''} saving={saving === c.key} onSet={(v) => onSet(c.key, v)} />
          );
        })}
      </PageSection>

      {showFileMgr && (
        <PageSection title="File Manager">
          <ConfigRow
            setting={advanced.find((c) => c.key === 'gui_file_manager')!}
            value={configs['gui_file_manager'] ?? 'auto'}
            saving={saving === 'gui_file_manager'}
            onSet={(v) => onSet('gui_file_manager', v)}
          />
          {configs['gui_file_manager'] === 'custom' && (
            <ConfigRow
              setting={advanced.find((c) => c.key === 'gui_file_manager_custom')!}
              value={configs['gui_file_manager_custom'] ?? ''}
              saving={saving === 'gui_file_manager_custom'}
              onSet={(v) => onSet('gui_file_manager_custom', v)}
            />
          )}
        </PageSection>
      )}

      {showTerminalMgr && (
        <PageSection title="Terminal File Manager">
          <ConfigRow
            setting={advanced.find((c) => c.key === 'gui_terminal_file_manager')!}
            value={configs['gui_terminal_file_manager'] ?? 'yazi'}
            saving={saving === 'gui_terminal_file_manager'}
            onSet={(v) => onSet('gui_terminal_file_manager', v)}
          />
          {configs['gui_terminal_file_manager'] === 'custom' && (
            <ConfigRow
              setting={advanced.find((c) => c.key === 'gui_terminal_file_manager_custom')!}
              value={configs['gui_terminal_file_manager_custom'] ?? ''}
              saving={saving === 'gui_terminal_file_manager_custom'}
              onSet={(v) => onSet('gui_terminal_file_manager_custom', v)}
            />
          )}
        </PageSection>
      )}

      <PageSection title="Known Settings">
        <div className="known-settings-card-body">
          <p className="known-settings-description">
            Read-only view of recognized configuration key/value pairs.
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
        </div>
      </PageSection>

      {weDebugError && !weDebugInfo && (
        <PageSection title="WE Backend Debug Info">
          <p className="config-desc" style={{ color: 'var(--warning)' }}>
            Debug info unavailable: {weDebugError}
          </p>
        </PageSection>
      )}

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
    </SettingsPageShell>
  );
}
