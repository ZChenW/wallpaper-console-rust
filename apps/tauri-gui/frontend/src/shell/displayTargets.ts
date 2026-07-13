import type { DisplayTarget } from './shellPreferences.ts';

export const ALL_DISPLAYS_SELECT_VALUE = 'all-displays';
const OUTPUT_SELECT_VALUE_PREFIX = 'output:';

export interface DisplayTargetOption {
  readonly label: string;
  readonly value: string;
}

export interface DisplayTargetModel {
  readonly hidden: boolean;
  readonly connectedOutputs: readonly string[];
  readonly selectedTarget: DisplayTarget;
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

export function resolveConnectedDisplayTarget(
  target: DisplayTarget,
  connectedOutputs: readonly string[],
): DisplayTarget {
  if (target.kind !== 'output') return { kind: 'allDisplays' };
  const output = target.output.trim();
  return output.length > 0 && normalizeDisplayOutputs(connectedOutputs).includes(output)
    ? { kind: 'output', output }
    : { kind: 'allDisplays' };
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
  return {
    hidden: outputs.length <= 1,
    connectedOutputs: outputs,
    selectedTarget: resolveConnectedDisplayTarget(savedTarget, outputs),
    options: [
      { label: 'All Displays', value: ALL_DISPLAYS_SELECT_VALUE },
      ...outputs.map((output) => ({
        label: output,
        value: displayTargetToSelectValue({ kind: 'output', output }),
      })),
    ],
  };
}
