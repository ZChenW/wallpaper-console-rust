import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
  type MouseEvent,
} from 'react';
import { ArrowLeft, Pencil, RefreshCw, Trash2, X } from 'lucide-react';

import { commandResultMessage } from '../api/feedback.ts';
import type { CommandResult, SourceDTO } from '../api/types';
import {
  useWallpaperSources,
  type AddSourceOutcome,
  type UseWallpaperSourcesOptions,
} from './useWallpaperSources';
import { trapDialogFocus } from './dialogFocus.ts';

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

export type SourcePanelPresentationPhase = 'open' | 'exiting';

export function beginSourcePanelExit(
  phase: SourcePanelPresentationPhase,
  reducedMotion: boolean,
): {
  readonly accepted: boolean;
  readonly next: SourcePanelPresentationPhase;
  readonly delayMs: number;
} {
  if (phase === 'exiting') return { accepted: false, next: 'exiting', delayMs: 0 };
  return {
    accepted: true,
    next: 'exiting',
    delayMs: reducedMotion ? 0 : 180,
  };
}

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
  readonly onBack?: () => void;
  readonly onClose: () => void;
  readonly onNotice: (notice: SourcePanelNotice) => void;
  readonly onScanStarted?: () => void;
  readonly onScanFinished?: () => void;
}

export interface SourcePanelViewProps {
  readonly open: boolean;
  readonly presentationPhase?: SourcePanelPresentationPhase;
  readonly sources: readonly SourceDTO[];
  readonly loading: boolean;
  readonly loadError: string | null;
  readonly pendingOperation: string | null;
  readonly removeCandidateId: number | null;
  readonly editingSourceId: number | null;
  readonly renameDraft: string;
  readonly onBack?: () => void;
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
  readonly successMessage: string | ((result: CommandResult) => string);
  readonly failureMessage: string;
  readonly onNotice: (notice: SourcePanelNotice) => void;
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
      onNotice({
        channel,
        severity: 'success',
        message: typeof successMessage === 'function'
          ? successMessage(result)
          : successMessage,
      });
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
      successMessage: (result) => commandResultMessage(
        result,
        'Wallpaper Engine scan finished',
      ),
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
    >
      <div className="source-panel__source-header">
        <div className="source-panel__source-identity">
          <div className="source-panel__name-row">
            {editing ? (
              <form
                className="source-panel__rename-form"
                data-source-action={`rename:${source.id}`}
                onSubmit={(event: FormEvent<HTMLFormElement>) => {
                  event.preventDefault();
                  const displayName = renameDraft.trim();
                  if (displayName) onRename(source.id, displayName);
                }}
              >
                <input
                  aria-label={`Alias for ${source.displayName}`}
                  autoComplete="off"
                  autoFocus
                  className="source-panel__input"
                  data-source-mutating={true}
                  disabled={busy}
                  maxLength={120}
                  name="displayName"
                  onBlur={() => {
                    if (!busy) onCancelRename();
                  }}
                  onChange={(event) => onChangeRenameDraft(event.currentTarget.value)}
                  onKeyDown={(event) => {
                    if (event.key !== 'Escape') return;
                    event.preventDefault();
                    event.stopPropagation();
                    onCancelRename();
                  }}
                  required
                  value={renameDraft}
                />
              </form>
            ) : (
              <span className="source-panel__name" id={sourceNameId}>{source.displayName}</span>
            )}
            <button
              aria-label={`Rename ${source.displayName}`}
              aria-pressed={editing}
              className="source-panel__icon-button"
              data-source-action={`start-rename:${source.id}`}
              data-source-mutating={true}
              disabled={busy}
              onClick={() => onStartRename(source)}
              title={`Rename ${source.displayName}`}
              type="button"
            >
              <Pencil aria-hidden="true" size={15} />
            </button>
          </div>
          <span className="source-panel__path" title={source.path}>{source.path}</span>
        </div>
        <span
          aria-label={`Availability: ${availabilityLabel(source)}`}
          className={`source-panel__badge source-panel__badge--${source.availability}`}
          data-source-availability={source.availability}
        >
          {availabilityLabel(source)}
        </span>
      </div>

      <span className="source-panel__badge">{sourceKindLabel(source)}</span>

      <div className="source-panel__actions">
        {source.kind === 'directory' ? (
          <label className="source-panel__switch-row">
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

        <div className="source-panel__icon-actions">
          <button
            aria-label={`Refresh ${source.displayName}`}
            className="source-panel__icon-button"
            data-source-action={`refresh:${source.id}`}
            data-source-mutating={true}
            disabled={busy}
            onClick={() => onRefresh(source.id)}
            title={`Refresh ${source.displayName}`}
            type="button"
          >
            <RefreshCw aria-hidden="true" size={15} />
          </button>
          <button
            aria-label={`Remove ${source.displayName}`}
            aria-describedby={editing ? undefined : sourceNameId}
            className="source-panel__icon-button source-panel__icon-button--danger"
            data-source-action={`request-remove:${source.id}`}
            data-source-mutating={true}
            disabled={busy}
            onClick={() => onRequestRemove(source.id)}
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
          className="source-panel__confirm"
          role="alertdialog"
        >
          <strong id={`remove-source-title-${source.id}`}>Remove {source.displayName}?</strong>
          <span>
            This only removes this source from the library index. It does not delete wallpaper files.
          </span>
          <div className="source-panel__actions">
            <button
              autoFocus
              className="source-panel__button"
              data-source-action={`cancel-remove:${source.id}`}
              onClick={onCancelRemove}
              type="button"
            >
              Cancel
            </button>
            <button
              className="source-panel__button source-panel__button--danger"
              data-source-action={`confirm-remove:${source.id}`}
              data-source-mutating={true}
              disabled={busy}
              onClick={() => onRemove(source.id)}
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
  presentationPhase = 'open',
  sources,
  loading,
  loadError,
  pendingOperation,
  removeCandidateId,
  editingSourceId,
  renameDraft,
  onBack,
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
    if (event.key === 'Tab') {
      trapDialogFocus(event, event.currentTarget);
      return;
    }
    if (event.key !== 'Escape') return;
    event.preventDefault();
    if (removeCandidateId !== null) onCancelRemove();
    else onClose();
  };
  const handleBackdropMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) onClose();
  };

  return (
    <div
      className={`source-panel__backdrop${onBack ? ' source-panel__backdrop--layered' : ''}`}
      data-layered={onBack ? true : undefined}
      onMouseDown={handleBackdropMouseDown}
    >
      <aside
        aria-busy={busy}
        aria-hidden={presentationPhase === 'exiting' ? true : undefined}
        aria-labelledby="source-panel-title"
        aria-modal="true"
        className="source-panel"
        data-presentation-phase={presentationPhase}
        inert={presentationPhase === 'exiting'}
        onKeyDown={handleKeyDown}
        role="dialog"
      >
        <header className="source-panel__header">
          <div className="source-panel__header-title">
            {onBack ? (
              <button
                aria-label="Back to settings"
                className="source-panel__close"
                disabled={presentationPhase === 'exiting' ? true : undefined}
                onClick={onBack}
                type="button"
              >
                <ArrowLeft aria-hidden="true" size={17} />
              </button>
            ) : null}
            <h2 autoFocus className="source-panel__title" id="source-panel-title" tabIndex={-1}>
              Wallpaper sources
            </h2>
          </div>
          <span
            aria-hidden="true"
            className="source-panel__drag-region"
            data-tauri-drag-region="deep"
          />
          <button
            aria-label="Close wallpaper sources"
            className="source-panel__close"
            disabled={presentationPhase === 'exiting' ? true : undefined}
            onClick={onClose}
            type="button"
          >
            <X aria-hidden="true" size={17} />
          </button>
        </header>

        <div className="source-panel__toolbar">
          <button
            className="source-panel__button source-panel__button--primary"
            data-source-action="add"
            data-source-mutating={true}
            disabled={busy}
            onClick={onAdd}
            type="button"
          >
            Add folder
          </button>
          <button
            className="source-panel__button"
            data-source-action="refresh-all"
            data-source-mutating={true}
            disabled={busy || loading || sources.length === 0}
            onClick={onRefreshAll}
            type="button"
          >
            Refresh all
          </button>
          <button
            className="source-panel__button"
            data-source-action="scan-wallpaper-engine"
            data-source-mutating={true}
            disabled={busy}
            onClick={onScanWallpaperEngine}
            type="button"
          >
            Scan Wallpaper Engine
          </button>
        </div>

        <div className="source-panel__content">
          {pendingOperation ? (
            <p aria-live="polite" className="source-panel__status" role="status">
              {pendingOperationLabel(pendingOperation)}
            </p>
          ) : null}

          {loading ? (
            <p aria-live="polite" className="source-panel__status" role="status">Loading sources…</p>
          ) : null}

          {loadError ? (
            <div className="source-panel__status source-panel__status--error" role="alert">
              <p className="source-panel__status-lead">Could not load sources: {loadError}</p>
              <button
                className="source-panel__button"
                disabled={loading || busy}
                onClick={onReload}
                type="button"
              >
                Retry
              </button>
            </div>
          ) : null}

          {!loading && !loadError && sources.length === 0 ? (
            <div className="source-panel__status source-panel__empty">
              <strong className="source-panel__empty-title">No wallpaper sources yet</strong>
              <span>Add a folder or scan Wallpaper Engine when you are ready.</span>
            </div>
          ) : null}

          {sources.length > 0 ? (
            <ul aria-label="Configured wallpaper sources" className="source-panel__list">
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
  onBack,
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

  const [prevOpen, setPrevOpen] = useState(open);
  const [shouldRender, setShouldRender] = useState(open);
  const [presentationPhase, setPresentationPhase] = useState<SourcePanelPresentationPhase>('open');
  const exitTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const visibility = useRef<SourcePanelVisibility>({ open: false, hasOpened: false });

  // Synchronously adjust state when open changes during render
  if (open !== prevOpen) {
    setPrevOpen(open);
    if (open) {
      if (exitTimerRef.current !== null) {
        clearTimeout(exitTimerRef.current);
        exitTimerRef.current = null;
      }
      setShouldRender(true);
      setPresentationPhase('open');

      const wasOpen = visibility.current.open;
      const transition = transitionSourcePanelVisibility(visibility.current, true);
      visibility.current = transition.next;
      if (transition.reload) void reload();
    } else {
      const wasOpen = visibility.current.open;
      const transition = transitionSourcePanelVisibility(visibility.current, false);
      visibility.current = transition.next;

      const reducedMotion = typeof window !== 'undefined'
        && window.matchMedia?.('(prefers-reduced-motion: reduce)').matches === true;

      setPresentationPhase('exiting');
      if (reducedMotion) {
        setShouldRender(false);
      } else {
        exitTimerRef.current = setTimeout(() => {
          exitTimerRef.current = null;
          setShouldRender(false);
        }, 180);
      }
    }
  }

  useEffect(() => () => {
    if (exitTimerRef.current !== null) clearTimeout(exitTimerRef.current);
  }, []);

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

  if (!shouldRender) return null;

  return (
    <SourcePanelView
      loadError={loadError}
      loading={loading}
      editingSourceId={renameEditor?.sourceId ?? null}
      onBack={onBack}
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
      open={shouldRender}
      pendingOperation={pendingOperation}
      presentationPhase={presentationPhase}
      renameDraft={renameEditor?.draft ?? ''}
      removeCandidateId={removeCandidateId}
      sources={sources}
    />
  );
}
