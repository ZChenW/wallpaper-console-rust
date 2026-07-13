import type { ChangeEvent } from 'react';
import type { DisplayTarget } from './shellPreferences.ts';
import {
  buildDisplayTargetModel,
  displayTargetFromSelectValue,
  displayTargetToSelectValue,
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
    onChange(decoded);
  };

  return (
    <select
      aria-label={ariaLabel}
      disabled={disabled}
      value={displayTargetToSelectValue(model.selectedTarget)}
      onChange={handleChange}
    >
      {model.options.map((option) => (
        <option key={option.value} value={option.value} disabled={option.disabled}>
          {option.label}
        </option>
      ))}
    </select>
  );
}
