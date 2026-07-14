import type { DisplayTarget } from './shellPreferences.ts';

export const ALL_DISPLAYS_SELECT_VALUE = 'all-displays';
const OUTPUT_SELECT_VALUE_PREFIX = 'output:';

export interface DisplayTargetOption {
  readonly label: string;
  readonly value: string;
  readonly disabled: boolean;
}

export interface DisplayTargetModel {
  readonly hidden: boolean;
  readonly connectedOutputs: readonly string[];
  readonly selectedTarget: DisplayTarget;
  readonly canApply: boolean;
  readonly options: readonly DisplayTargetOption[];
}

export function normalizeDisplayOutputs(outputs: readonly string[]): string[] {
  const seen = new Set<string>();
  const normalized: string[] = [];
  for (const candidate of outputs) {
    if (typeof candidate !== 'string') continue;
    const output = candidate.trim();
    if (output.length === 0 || seen.has(output)) continue;
    seen.add(output);
    normalized.push(output);
  }
  return normalized;
}

export function normalizeDisplayTarget(target: DisplayTarget): DisplayTarget {
  if (target.kind !== 'output') return { kind: 'allDisplays' };
  const output = target.output.trim();
  return output.length > 0 ? { kind: 'output', output } : { kind: 'allDisplays' };
}

export function displayTargetToSelectValue(target: DisplayTarget): string {
  if (target.kind !== 'output') return ALL_DISPLAYS_SELECT_VALUE;
  const output = target.output.trim();
  return output.length > 0
    ? `${OUTPUT_SELECT_VALUE_PREFIX}${encodeURIComponent(output)}`
    : ALL_DISPLAYS_SELECT_VALUE;
}

export function displayTargetFromSelectValue(value: string): DisplayTarget {
  if (value === ALL_DISPLAYS_SELECT_VALUE) return { kind: 'allDisplays' };
  if (!value.startsWith(OUTPUT_SELECT_VALUE_PREFIX)) return { kind: 'allDisplays' };
  try {
    const output = decodeURIComponent(value.slice(OUTPUT_SELECT_VALUE_PREFIX.length)).trim();
    return output.length > 0 ? { kind: 'output', output } : { kind: 'allDisplays' };
  } catch {
    return { kind: 'allDisplays' };
  }
}

export function buildDisplayTargetModel(
  connectedOutputs: readonly string[],
  savedTarget: DisplayTarget,
): DisplayTargetModel {
  const outputs = normalizeDisplayOutputs(connectedOutputs);
  const selectedTarget = normalizeDisplayTarget(savedTarget);
  const canApply = outputs.length > 0 && (
    selectedTarget.kind !== 'output'
    || outputs.includes(selectedTarget.output)
  );
  const disconnectedOutput = selectedTarget.kind === 'output' && !canApply
    ? selectedTarget.output
    : null;
  const options: DisplayTargetOption[] = [
    { label: 'All Displays', value: ALL_DISPLAYS_SELECT_VALUE, disabled: false },
    ...outputs.map((output) => ({
      label: output,
      value: displayTargetToSelectValue({ kind: 'output', output }),
      disabled: false,
    })),
  ];
  if (disconnectedOutput !== null) {
    options.push({
      label: `${disconnectedOutput} (Disconnected)`,
      value: displayTargetToSelectValue({ kind: 'output', output: disconnectedOutput }),
      disabled: true,
    });
  }
  return {
    hidden: outputs.length <= 1 && disconnectedOutput === null,
    connectedOutputs: outputs,
    selectedTarget,
    canApply,
    options,
  };
}
