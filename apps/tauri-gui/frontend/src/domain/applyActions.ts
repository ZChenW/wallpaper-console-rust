import type {
  ApplyActionDTO,
  ApplyActionKind,
  ApplyRequestKind,
  WallpaperDTO,
} from '../api/bridge';

export type NormalizedApplyAction = {
  kind: ApplyActionKind;
  label: string;
  enabled: boolean;
  reason?: string;
};

const VALID_KINDS: ApplyActionKind[] = [
  'apply',
  'retry_backend_apply',
  'apply_preview',
  'open_folder',
  'copy_workshop_id',
];

function isValidAction(a: ApplyActionDTO): a is ApplyActionDTO & { kind: ApplyActionKind; label: string; enabled: boolean } {
  if (!a || !a.kind || !a.label) return false;
  if (typeof a.enabled !== 'boolean') return false;
  if (!VALID_KINDS.includes(a.kind)) return false;
  return true;
}

export function normalizeApplyActions(entry: WallpaperDTO): NormalizedApplyAction[] {
  if (entry.applyActions && entry.applyActions.length > 0) {
    return entry.applyActions.filter(isValidAction).map((a) => ({
      kind: a.kind,
      label: a.label,
      enabled: a.enabled,
      reason: a.reason,
    }));
  }

  const actions: NormalizedApplyAction[] = [];
  const canOpenFolder = Boolean(entry.path);
  const canCopyWorkshopId = Boolean(entry.workshopId);

  if (entry.type === 'image' || entry.type === 'gif' || entry.type === 'video') {
    actions.push({ kind: 'apply', label: 'Apply', enabled: true });
    if (canOpenFolder) actions.push({ kind: 'open_folder', label: 'Open folder', enabled: true });
  } else if (entry.type === 'we_scene') {
    actions.push({ kind: 'apply', label: 'Apply', enabled: true });
    if (canOpenFolder) actions.push({ kind: 'open_folder', label: 'Open folder', enabled: true });
    if (canCopyWorkshopId) actions.push({ kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true });
  } else if (entry.type === 'we_web') {
    if (canOpenFolder) actions.push({ kind: 'open_folder', label: 'Open folder', enabled: true });
    if (canCopyWorkshopId) actions.push({ kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true });
  } else if (entry.type === 'unsupported') {
    if (canOpenFolder) actions.push({ kind: 'open_folder', label: 'Open folder', enabled: true });
    if (canCopyWorkshopId) actions.push({ kind: 'copy_workshop_id', label: 'Copy Workshop ID', enabled: true });
  }

  return actions;
}

export function hasEnabledAction(entry: WallpaperDTO, kind: ApplyActionKind): boolean {
  return normalizeApplyActions(entry).some((a) => a.kind === kind && a.enabled);
}

export function isApplyAvailable(entry: WallpaperDTO): boolean {
  return hasEnabledAction(entry, 'apply');
}

/** The direct card gesture, including the one explicit recovery action. */
export function primaryApplyKind(entry: WallpaperDTO): ApplyRequestKind | null {
  if (hasEnabledAction(entry, 'apply')) return 'apply';
  if (hasEnabledAction(entry, 'retry_backend_apply')) return 'retry_backend_apply';
  return null;
}

export function getActionReason(entry: WallpaperDTO, kind: ApplyActionKind): string | undefined {
  const a = normalizeApplyActions(entry).find((a) => a.kind === kind);
  return a?.reason;
}
