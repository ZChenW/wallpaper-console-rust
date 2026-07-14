import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent,
  type KeyboardEvent,
} from 'react';
import { Pencil, RefreshCw, Trash2 } from 'lucide-react';

import type { CommandResult, SourceDTO } from '../api/types';
import {
  useWallpaperSources,
  type AddSourceOutcome,
  type UseWallpaperSourcesOptions,
} from './useWallpaperSources';

export type SourcePanelNotice = {
  readonly channel: 'settings' | 'scan';
  readonly severity: 'success' | 'info' | 'warning' | 'error';
  readonly message: string;
  readonly technicalDetails?: string;
};

export type SourcePanelVisibility = {
  readonly open: boolean;
  readonly hasOpened: boolean;
};

type RenameEditor = { readonly sourceId: number; readonly draft: string };

export function renameEditorAfterResult(
  editor: RenameEditor | null,
  sourceId: number,
  succeeded: boolean,
): RenameEditor | null {
  return succeeded && editor?.sourceId === sourceId ? null : editor;
}

export function transitionSourcePanelVisibility(
  previous: SourcePanelVisibility,
  open: boolean,
): { readonly next: SourcePanelVisibility; readonly reload: boolean } {
  return {
    next: {
      open,
      hasOpened: previous.hasOpened || open,
    },
    reload: open && !previous.open && previous.hasOpened,
  };
}

export interface SourcePanelProps extends UseWallpaperSourcesOptions {
  readonly open: boolean;
  readonly onClose: () => void;
  readonly onNotice: (notice: SourcePanelNotice) => void;
  readonly onScanStarted?: () => void;
  readonly onScanFinished?: () => void;
}

export interface SourcePanelViewProps {
  readonly open: boolean;
  readonly sources: readonly SourceDTO[];
  readonly loading: boolean;
  readonly loadError: string | null;
  readonly pendingOperation: string | null;
  readonly removeCandidateId: number | null;
  readonly editingSourceId: number | null;
  readonly renameDraft: string;
  readonly onClose: () => void;
  readonly onReload: () => void;
  readonly onAdd: () => void;
  readonly onRefreshAll: () => void;
  readonly onScanWallpaperEngine: () => void;
  readonly onRename: (id: number, displayName: string) => void;
  readonly onStartRename: (source: SourceDTO) => void;
  readonly onChangeRenameDraft: (displayName: string) => void;
  readonly onCancelRename: () => void;
  readonly onSetRecursive: (id: number, recursive: boolean) => void;
  readonly onRefresh: (id: number) => void;
  readonly onRequestRemove: (id: number) => void;
  readonly onCancelRemove: () => void;
  readonly onRemove: (id: number) => void;
}

type RunCommandOptions = {
  readonly action: () => Promise<CommandResult>;
  readonly channel: SourcePanelNotice['channel'];
  readonly successMessage: string;
  readonly failureMessage: string;
  readonly onNotice: (notice: SourcePanelNotice) => void;
};

const backdropStyle: CSSProperties = {
  position: 'fixed',
  inset: 0,
  zIndex: 800,
  display: 'flex',
  justifyContent: 'flex-end',
  background: 'rgb(0 0 0 / 30%)',
};

const panelStyle: CSSProperties = {
  display: 'flex',
  width: 'min(34rem, 100vw)',
  height: '100%',
  flexDirection: 'column',
  overflow: 'hidden',
  borderInlineStart: '1px solid color-mix(in srgb, currentColor 16%, transparent)',
  background: 'Canvas',
  color: 'CanvasText',
  boxShadow: '-1rem 0 3rem rgb(0 0 0 / 22%)',
};

const headerStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: '1rem',
  padding: '1rem 1.1rem 0.8rem',
  borderBottom: '1px solid color-mix(in srgb, currentColor 12%, transparent)',
};

const titleStyle: CSSProperties = {
  margin: 0,
  fontSize: '1.05rem',
};

const closeStyle: CSSProperties = {
  width: '2rem',
  height: '2rem',
  padding: 0,
  border: 0,
  borderRadius: '0.45rem',
  background: 'transparent',
  color: 'inherit',
  cursor: 'pointer',
  font: 'inherit',
  fontSize: '1.3rem',
};

const toolbarStyle: CSSProperties = {
  display: 'flex',
  flexWrap: 'wrap',
  gap: '0.55rem',
  padding: '0.8rem 1.1rem',
};

const buttonStyle: CSSProperties = {
  minHeight: '2.1rem',
  padding: '0.35rem 0.7rem',
  border: '1px solid color-mix(in srgb, currentColor 18%, transparent)',
  borderRadius: '0.5rem',
  background: 'transparent',
  color: 'inherit',
  cursor: 'pointer',
  font: 'inherit',
  fontSize: '0.8rem',
};

const primaryButtonStyle: CSSProperties = {
  ...buttonStyle,
  background: 'color-mix(in srgb, AccentColor 16%, transparent)',
};

const dangerButtonStyle: CSSProperties = {
  ...buttonStyle,
  borderColor: 'color-mix(in srgb, #ff6b6b 55%, transparent)',
  color: '#d94b4b',
};

const contentStyle: CSSProperties = {
  minHeight: 0,
  flex: 1,
  overflow: 'auto',
  padding: '0 1.1rem 1.1rem',
};

const statusStyle: CSSProperties = {
  margin: '0 0 0.75rem',
  padding: '0.7rem 0.8rem',
  borderRadius: '0.55rem',
  background: 'color-mix(in srgb, currentColor 6%, transparent)',
  fontSize: '0.8rem',
};

const listStyle: CSSProperties = {
  display: 'grid',
  gap: '0.7rem',
  margin: 0,
  padding: 0,
  listStyle: 'none',
};

const rowStyle: CSSProperties = {
  display: 'grid',
  gap: '0.65rem',
  padding: '0.85rem',
  border: '1px solid color-mix(in srgb, currentColor 13%, transparent)',
  borderRadius: '0.7rem',
  background: 'color-mix(in srgb, CanvasText 2.5%, Canvas)',
};

const rowHeaderStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: 'minmax(0, 1fr) auto',
  alignItems: 'start',
  gap: '0.7rem',
};

const sourceNameStyle: CSSProperties = {
  display: 'block',
  overflow: 'hidden',
  fontSize: '0.92rem',
  fontWeight: 650,
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
};

const sourceNameRowStyle: CSSProperties = {
  display: 'flex',
  minWidth: 0,
  alignItems: 'center',
  gap: '0.35rem',
};

const sourcePathStyle: CSSProperties = {
  display: 'block',
  marginTop: '0.2rem',
  overflowWrap: 'anywhere',
  fontSize: '0.72rem',
  opacity: 0.68,
};

const badgeStyle: CSSProperties = {
  display: 'inline-flex',
  width: 'fit-content',
  padding: '0.16rem 0.42rem',
  borderRadius: '999px',
  background: 'color-mix(in srgb, currentColor 7%, transparent)',
  fontSize: '0.68rem',
};

const rowActionsStyle: CSSProperties = {
  display: 'flex',
  flexWrap: 'wrap',
  alignItems: 'center',
  gap: '0.45rem',
};

const iconActionsStyle: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: '0.35rem',
  marginInlineStart: 'auto',
};

const iconButtonStyle: CSSProperties = {
  display: 'inline-flex',
  width: '2rem',
  height: '2rem',
  alignItems: 'center',
  justifyContent: 'center',
  padding: 0,
  border: '1px solid color-mix(in srgb, currentColor 18%, transparent)',
  borderRadius: '0.45rem',
  background: 'transparent',
  color: 'inherit',
  cursor: 'pointer',
};

const renameFormStyle: CSSProperties = {
  minWidth: 0,
  flex: 1,
};

const inputStyle: CSSProperties = {
  minWidth: 0,
  minHeight: '2rem',
  padding: '0.3rem 0.5rem',
  border: '1px solid color-mix(in srgb, currentColor 18%, transparent)',
  borderRadius: '0.45rem',
  background: 'Canvas',
  color: 'CanvasText',
  font: 'inherit',
};

const confirmationStyle: CSSProperties = {
  display: 'grid',
  gap: '0.55rem',
  padding: '0.7rem',
  border: '1px solid color-mix(in srgb, #ff6b6b 45%, transparent)',
  borderRadius: '0.55rem',
  background: 'color-mix(in srgb, #ff6b6b 8%, Canvas)',
  fontSize: '0.78rem',
};

function technicalDetailsForResult(result: CommandResult): string {
  return [
    result.error?.message,
    result.error?.suggestion,
    result.error?.detail,
    result.stderr,
    result.stdout,
    `Exit code: ${result.exitCode}`,
  ].filter((part): part is string => Boolean(part)).join('\n');
}

function technicalDetailsForError(error: unknown): string {
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === 'string' && error) return error;
  return String(error);
}

async function runCommand({
  action,
  channel,
  successMessage,
  failureMessage,
  onNotice,
}: RunCommandOptions): Promise<boolean> {
  try {
    const result = await action();
    if (result.success) {
      onNotice({ channel, severity: 'success', message: successMessage });
      return true;
    }
    onNotice({
      channel,
      severity: 'error',
      message: failureMessage,
      technicalDetails: technicalDetailsForResult(result),
    });
  } catch (error) {
    onNotice({
      channel,
      severity: 'error',
      message: failureMessage,
      technicalDetails: technicalDetailsForError(error),
    });
  }
  return false;
}

export async function runAddSourceAction(
  action: () => Promise<AddSourceOutcome>,
  onNotice: (notice: SourcePanelNotice) => void,
): Promise<boolean | null> {
  try {
    const outcome = await action();
    if (outcome.kind === 'cancelled') return null;
    if (outcome.result.success) {
      onNotice({ channel: 'settings', severity: 'success', message: 'Folder added' });
      return true;
    }
    onNotice({
      channel: 'settings',
      severity: 'error',
      message: 'Could not finish adding folder',
      technicalDetails: technicalDetailsForResult(outcome.result),
    });
  } catch (error) {
    onNotice({
      channel: 'settings',
      severity: 'error',
      message: 'Could not add folder',
      technicalDetails: technicalDetailsForError(error),
    });
  }
  return false;
}

export async function runWallpaperEngineScanAction(
  action: () => Promise<CommandResult>,
  onNotice: (notice: SourcePanelNotice) => void,
  onStarted?: () => void,
  onFinished?: () => void,
): Promise<boolean> {
  onStarted?.();
  onNotice({ channel: 'scan', severity: 'info', message: 'Scanning Wallpaper Engine' });
  try {
    return await runCommand({
      action,
      channel: 'scan',
      successMessage: 'Wallpaper Engine scan finished',
      failureMessage: 'Could not scan Wallpaper Engine',
      onNotice,
    });
  } finally {
    onFinished?.();
  }
}

export async function runSourceRefreshAction(
  action: () => Promise<CommandResult>,
  onNotice: (notice: SourcePanelNotice) => void,
  onStarted?: () => void,
  onFinished?: () => void,
): Promise<boolean> {
  onStarted?.();
  onNotice({ channel: 'scan', severity: 'info', message: 'Refreshing source' });
  try {
    return await runCommand({
      action,
      channel: 'scan',
      successMessage: 'Source refresh finished',
      failureMessage: 'Could not refresh source',
      onNotice,
    });
  } finally {
    onFinished?.();
  }
}

export async function runAllSourcesRefreshAction(
  action: () => Promise<CommandResult>,
  onNotice: (notice: SourcePanelNotice) => void,
  onStarted?: () => void,
  onFinished?: () => void,
): Promise<boolean> {
  onStarted?.();
  onNotice({ channel: 'scan', severity: 'info', message: 'Refreshing all sources' });
  try {
    return await runCommand({
      action,
      channel: 'scan',
      successMessage: 'All sources refreshed',
      failureMessage: 'Could not refresh all sources',
      onNotice,
    });
  } finally {
    onFinished?.();
  }
}

function pendingOperationLabel(operation: string): string {
  if (operation === 'add') return 'Adding source…';
  if (operation === 'scanWallpaperEngine') return 'Scanning Wallpaper Engine…';
  if (operation === 'refreshAll') return 'Refreshing all sources…';
  if (operation.startsWith('rename:')) return 'Renaming source…';
  if (operation.startsWith('recursive:')) return 'Updating scan depth…';
  if (operation.startsWith('refresh:')) return 'Refreshing source…';
  if (operation.startsWith('remove:')) return 'Removing source…';
  return 'Updating sources…';
}

function sourceKindLabel(source: SourceDTO): string {
  return source.kind === 'wallpaper_engine_workshop' ? 'Wallpaper Engine Workshop' : 'Directory';
}

function availabilityLabel(source: SourceDTO): string {
  if (source.availability === 'available') return 'Available';
  if (source.availability === 'offline') return 'Offline — indexed wallpapers are kept';
  return 'Availability unknown';
}

function availabilityStyle(availability: SourceDTO['availability']): CSSProperties {
  if (availability === 'available') {
    return { color: '#2e9b59', background: 'rgb(46 155 89 / 16%)' };
  }
  if (availability === 'offline') {
    return { color: '#d94b4b', background: 'rgb(217 75 75 / 16%)' };
  }
  return { color: '#b78700', background: 'rgb(183 135 0 / 16%)' };
}

function SourceRow({
  source,
  busy,
  confirmingRemoval,
  editing,
  renameDraft,
  onRename,
  onStartRename,
  onChangeRenameDraft,
  onCancelRename,
  onSetRecursive,
  onRefresh,
  onRequestRemove,
  onCancelRemove,
  onRemove,
}: {
  readonly source: SourceDTO;
  readonly busy: boolean;
  readonly confirmingRemoval: boolean;
  readonly editing: boolean;
  readonly renameDraft: string;
  readonly onRename: SourcePanelViewProps['onRename'];
  readonly onStartRename: SourcePanelViewProps['onStartRename'];
  readonly onChangeRenameDraft: SourcePanelViewProps['onChangeRenameDraft'];
  readonly onCancelRename: SourcePanelViewProps['onCancelRename'];
  readonly onSetRecursive: SourcePanelViewProps['onSetRecursive'];
  readonly onRefresh: SourcePanelViewProps['onRefresh'];
  readonly onRequestRemove: SourcePanelViewProps['onRequestRemove'];
  readonly onCancelRemove: SourcePanelViewProps['onCancelRemove'];
  readonly onRemove: SourcePanelViewProps['onRemove'];
}) {
  const sourceNameId = `source-name-${source.id}`;

  return (
    <li
      className={`source-panel__source source-panel__source--${source.availability}`}
      data-source-id={source.id}
      style={rowStyle}
    >
      <div style={rowHeaderStyle}>
        <div>
          <div style={sourceNameRowStyle}>
            {editing ? (
              <form
                data-source-action={`rename:${source.id}`}
                onSubmit={(event: FormEvent<HTMLFormElement>) => {
                  event.preventDefault();
                  const displayName = renameDraft.trim();
                  if (displayName) onRename(source.id, displayName);
                }}
                style={renameFormStyle}
              >
                <input
                  aria-label={`Alias for ${source.displayName}`}
                  autoComplete="off"
                  autoFocus
                  data-source-mutating={true}
                  disabled={busy}
                  maxLength={120}
                  name="displayName"
                  onChange={(event) => onChangeRenameDraft(event.currentTarget.value)}
                  onKeyDown={(event) => {
                    if (event.key !== 'Escape') return;
                    event.preventDefault();
                    event.stopPropagation();
                    onCancelRename();
                  }}
                  required
                  style={{ ...inputStyle, width: '100%' }}
                  value={renameDraft}
                />
              </form>
            ) : (
              <span id={sourceNameId} style={{ ...sourceNameStyle, minWidth: 0 }}>{source.displayName}</span>
            )}
            <button
              aria-label={`Rename ${source.displayName}`}
              aria-pressed={editing}
              data-source-action={`start-rename:${source.id}`}
              data-source-mutating={true}
              disabled={busy}
              onClick={() => onStartRename(source)}
              style={{ ...iconButtonStyle, flex: '0 0 auto' }}
              title={`Rename ${source.displayName}`}
              type="button"
            >
              <Pencil aria-hidden="true" size={15} />
            </button>
          </div>
          <span style={sourcePathStyle} title={source.path}>{source.path}</span>
        </div>
        <span
          aria-label={`Availability: ${availabilityLabel(source)}`}
          data-source-availability={source.availability}
          style={{ ...badgeStyle, ...availabilityStyle(source.availability) }}
        >
          {availabilityLabel(source)}
        </span>
      </div>

      <span style={badgeStyle}>{sourceKindLabel(source)}</span>

      <div style={rowActionsStyle}>
        {source.kind === 'directory' ? (
          <label style={{ display: 'inline-flex', alignItems: 'center', gap: '0.35rem', fontSize: '0.78rem' }}>
            <input
              aria-label={`Scan ${source.displayName} recursively`}
              checked={source.recursive}
              data-source-action={`recursive:${source.id}`}
              data-source-mutating={true}
              disabled={busy}
              onChange={(event) => onSetRecursive(source.id, event.currentTarget.checked)}
              role="switch"
              type="checkbox"
            />
            Include subfolders
          </label>
        ) : null}

        <div style={iconActionsStyle}>
          <button
            aria-label={`Refresh ${source.displayName}`}
            data-source-action={`refresh:${source.id}`}
            data-source-mutating={true}
            disabled={busy}
            onClick={() => onRefresh(source.id)}
            style={iconButtonStyle}
            title={`Refresh ${source.displayName}`}
            type="button"
          >
            <RefreshCw aria-hidden="true" size={15} />
          </button>
          <button
            aria-label={`Remove ${source.displayName}`}
            aria-describedby={editing ? undefined : sourceNameId}
            data-source-action={`request-remove:${source.id}`}
            data-source-mutating={true}
            disabled={busy}
            onClick={() => onRequestRemove(source.id)}
            style={{ ...iconButtonStyle, color: '#d94b4b' }}
            title={`Remove ${source.displayName}`}
            type="button"
          >
            <Trash2 aria-hidden="true" size={15} />
          </button>
        </div>
      </div>

      {confirmingRemoval ? (
        <section
          aria-labelledby={`remove-source-title-${source.id}`}
          aria-modal="false"
          role="alertdialog"
          style={confirmationStyle}
        >
          <strong id={`remove-source-title-${source.id}`}>Remove {source.displayName}?</strong>
          <span>
            This only removes this source from the library index. It does not delete wallpaper files.
          </span>
          <div style={rowActionsStyle}>
            <button
              data-source-action={`cancel-remove:${source.id}`}
              onClick={onCancelRemove}
              style={buttonStyle}
              type="button"
            >
              Cancel
            </button>
            <button
              data-source-action={`confirm-remove:${source.id}`}
              data-source-mutating={true}
              disabled={busy}
              onClick={() => onRemove(source.id)}
              style={dangerButtonStyle}
              type="button"
            >
              Remove source
            </button>
          </div>
        </section>
      ) : null}
    </li>
  );
}

export function SourcePanelView({
  open,
  sources,
  loading,
  loadError,
  pendingOperation,
  removeCandidateId,
  editingSourceId,
  renameDraft,
  onClose,
  onReload,
  onAdd,
  onRefreshAll,
  onScanWallpaperEngine,
  onRename,
  onStartRename,
  onChangeRenameDraft,
  onCancelRename,
  onSetRecursive,
  onRefresh,
  onRequestRemove,
  onCancelRemove,
  onRemove,
}: SourcePanelViewProps) {
  if (!open) return null;

  const busy = pendingOperation !== null;
  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (event.key !== 'Escape') return;
    if (removeCandidateId !== null) onCancelRemove();
    else onClose();
  };

  return (
    <div className="source-panel__backdrop" style={backdropStyle}>
      <aside
        aria-busy={busy}
        aria-label="Wallpaper sources"
        aria-modal="true"
        className="source-panel"
        onKeyDown={handleKeyDown}
        role="dialog"
        style={panelStyle}
      >
        <header style={headerStyle}>
          <h2 style={titleStyle}>Wallpaper sources</h2>
          <button autoFocus aria-label="Close wallpaper sources" onClick={onClose} style={closeStyle} type="button">
            <span aria-hidden="true">×</span>
          </button>
        </header>

        <div style={toolbarStyle}>
          <button
            data-source-action="add"
            data-source-mutating={true}
            disabled={busy}
            onClick={onAdd}
            style={primaryButtonStyle}
            type="button"
          >
            Add folder
          </button>
          <button
            data-source-action="refresh-all"
            data-source-mutating={true}
            disabled={busy || loading || sources.length === 0}
            onClick={onRefreshAll}
            style={buttonStyle}
            type="button"
          >
            Refresh all
          </button>
          <button
            data-source-action="scan-wallpaper-engine"
            data-source-mutating={true}
            disabled={busy}
            onClick={onScanWallpaperEngine}
            style={buttonStyle}
            type="button"
          >
            Scan Wallpaper Engine
          </button>
        </div>

        <div style={contentStyle}>
          {pendingOperation ? (
            <p aria-live="polite" role="status" style={statusStyle}>
              {pendingOperationLabel(pendingOperation)}
            </p>
          ) : null}

          {loading ? <p aria-live="polite" role="status" style={statusStyle}>Loading sources…</p> : null}

          {loadError ? (
            <div role="alert" style={{ ...statusStyle, borderInlineStart: '0.2rem solid #ff6b6b' }}>
              <p style={{ margin: '0 0 0.5rem' }}>Could not load sources: {loadError}</p>
              <button disabled={loading || busy} onClick={onReload} style={buttonStyle} type="button">
                Retry
              </button>
            </div>
          ) : null}

          {!loading && !loadError && sources.length === 0 ? (
            <div style={{ ...statusStyle, paddingBlock: '1.2rem', textAlign: 'center' }}>
              <strong style={{ display: 'block', marginBottom: '0.35rem' }}>No wallpaper sources yet</strong>
              <span>Add a folder or scan Wallpaper Engine when you are ready.</span>
            </div>
          ) : null}

          {sources.length > 0 ? (
            <ul aria-label="Configured wallpaper sources" style={listStyle}>
              {sources.map((source) => (
                <SourceRow
                  busy={busy}
                  confirmingRemoval={removeCandidateId === source.id}
                  editing={editingSourceId === source.id}
                  key={source.id}
                  onCancelRename={onCancelRename}
                  onChangeRenameDraft={onChangeRenameDraft}
                  onCancelRemove={onCancelRemove}
                  onRefresh={onRefresh}
                  onRemove={onRemove}
                  onRename={onRename}
                  onStartRename={onStartRename}
                  onRequestRemove={onRequestRemove}
                  onSetRecursive={onSetRecursive}
                  renameDraft={renameDraft}
                  source={source}
                />
              ))}
            </ul>
          ) : null}
        </div>
      </aside>
    </div>
  );
}

export function SourcePanel({
  open,
  onClose,
  onNotice,
  onScanStarted,
  onScanFinished,
  sourceApi,
  onLibraryChanged,
}: SourcePanelProps) {
  const {
    sources,
    loading,
    loadError,
    pendingOperation,
    reload,
    addFromPicker,
    rename,
    setRecursive,
    refresh,
    refreshAll,
    remove,
    scanWallpaperEngine,
  } = useWallpaperSources({
    sourceApi,
    onLibraryChanged,
    onScanStarted,
    onScanFinished,
  });
  const [removeCandidateId, setRemoveCandidateId] = useState<number | null>(null);
  const [renameEditor, setRenameEditor] = useState<RenameEditor | null>(null);
  const visibility = useRef<SourcePanelVisibility>({ open: false, hasOpened: false });

  useEffect(() => {
    const transition = transitionSourcePanelVisibility(visibility.current, open);
    visibility.current = transition.next;
    if (transition.reload) void reload();
  }, [open, reload]);

  useEffect(() => {
    if (removeCandidateId !== null && !sources.some((source) => source.id === removeCandidateId)) {
      setRemoveCandidateId(null);
    }
  }, [removeCandidateId, sources]);

  useEffect(() => {
    if (renameEditor !== null && !sources.some((source) => source.id === renameEditor.sourceId)) {
      setRenameEditor(null);
    }
  }, [renameEditor, sources]);

  const handleAdd = useCallback(() => {
    void runAddSourceAction(addFromPicker, onNotice);
  }, [addFromPicker, onNotice]);

  const handleRename = useCallback((id: number, displayName: string) => {
    void runCommand({
      action: () => rename(id, displayName),
      channel: 'settings',
      successMessage: 'Source renamed',
      failureMessage: 'Could not rename source',
      onNotice,
    }).then((succeeded) => {
      setRenameEditor((current) => renameEditorAfterResult(current, id, succeeded));
    });
  }, [onNotice, rename]);

  const handleSetRecursive = useCallback((id: number, recursive: boolean) => {
    void runCommand({
      action: () => setRecursive(id, recursive),
      channel: 'settings',
      successMessage: 'Source scan depth updated',
      failureMessage: 'Could not update source scan depth',
      onNotice,
    });
  }, [onNotice, setRecursive]);

  const handleRefresh = useCallback((id: number) => {
    void runSourceRefreshAction(
      () => refresh(id),
      onNotice,
      onScanStarted,
      onScanFinished,
    );
  }, [onNotice, onScanFinished, onScanStarted, refresh]);

  const handleRefreshAll = useCallback(() => {
    void runAllSourcesRefreshAction(
      refreshAll,
      onNotice,
      onScanStarted,
      onScanFinished,
    );
  }, [onNotice, onScanFinished, onScanStarted, refreshAll]);

  const handleRemove = useCallback((id: number) => {
    void runCommand({
      action: () => remove(id),
      channel: 'settings',
      successMessage: 'Source removed from library',
      failureMessage: 'Could not remove source',
      onNotice,
    }).then((removed) => {
      if (removed) setRemoveCandidateId(null);
    });
  }, [onNotice, remove]);

  const handleScanWallpaperEngine = useCallback(() => {
    void runWallpaperEngineScanAction(
      scanWallpaperEngine,
      onNotice,
      onScanStarted,
      onScanFinished,
    );
  }, [onNotice, onScanFinished, onScanStarted, scanWallpaperEngine]);

  return (
    <SourcePanelView
      loadError={loadError}
      loading={loading}
      editingSourceId={renameEditor?.sourceId ?? null}
      onAdd={handleAdd}
      onCancelRename={() => setRenameEditor(null)}
      onCancelRemove={() => setRemoveCandidateId(null)}
      onChangeRenameDraft={(draft) => setRenameEditor((current) => current ? { ...current, draft } : current)}
      onClose={onClose}
      onRefresh={handleRefresh}
      onRefreshAll={handleRefreshAll}
      onReload={() => { void reload(); }}
      onRemove={handleRemove}
      onRename={handleRename}
      onRequestRemove={setRemoveCandidateId}
      onScanWallpaperEngine={handleScanWallpaperEngine}
      onSetRecursive={handleSetRecursive}
      onStartRename={(source) => setRenameEditor({ sourceId: source.id, draft: source.displayName })}
      open={open}
      pendingOperation={pendingOperation}
      renameDraft={renameEditor?.draft ?? ''}
      removeCandidateId={removeCandidateId}
      sources={sources}
    />
  );
}
