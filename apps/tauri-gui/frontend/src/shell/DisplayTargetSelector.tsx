import SelectField from '../components/SelectField.tsx';
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

  const options = [...model.options];
  if (value.kind === 'outputs' && value.outputs.length > 0) {
    const multiValue = displayTargetToSelectValue(value);
    if (!options.some((option) => option.value === multiValue)) {
      options.splice(1, 0, {
        label: `${value.outputs.length} displays`,
        value: multiValue,
        disabled: false,
      });
    }
  }

  return (
    <SelectField
      aria-label={ariaLabel}
      disabled={disabled}
      value={displayTargetToSelectValue(model.selectedTarget)}
      options={options}
      onValueChange={(next) => onChange(displayTargetFromSelectValue(next))}
      variant="compact"
    />
  );
}
