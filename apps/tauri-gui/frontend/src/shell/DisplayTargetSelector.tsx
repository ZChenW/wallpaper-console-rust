import type { ChangeEvent } from 'react';
import type { DisplayTarget } from './shellPreferences.ts';
import {
  buildDisplayTargetModel,
  displayTargetFromSelectValue,
  displayTargetToSelectValue,
  resolveConnectedDisplayTarget,
} from './displayTargets.ts';

export interface DisplayTargetSelectorProps {
  readonly connectedOutputs: readonly string[];
  readonly value: DisplayTarget;
  readonly onChange: (target: DisplayTarget) => void;
  readonly ariaLabel?: string;
  readonly disabled?: boolean;
}

export default function DisplayTargetSelector({
  connectedOutputs,
  value,
  onChange,
  ariaLabel = 'Display target',
  disabled = false,
}: DisplayTargetSelectorProps) {
  const model = buildDisplayTargetModel(connectedOutputs, value);
  if (model.hidden) return null;

  const handleChange = (event: ChangeEvent<HTMLSelectElement>) => {
    const decoded = displayTargetFromSelectValue(event.currentTarget.value);
    onChange(resolveConnectedDisplayTarget(decoded, model.connectedOutputs));
  };

  return (
    <select
      aria-label={ariaLabel}
      disabled={disabled}
      value={displayTargetToSelectValue(model.selectedTarget)}
      onChange={handleChange}
    >
      {model.options.map((option) => (
        <option key={option.value} value={option.value}>{option.label}</option>
      ))}
    </select>
  );
}
