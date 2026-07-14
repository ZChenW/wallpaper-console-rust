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

  const handleChange = (value: string) => {
    const decoded = displayTargetFromSelectValue(value);
    onChange(decoded);
  };

  return (
    <SelectField
      aria-label={ariaLabel}
      disabled={disabled}
      value={displayTargetToSelectValue(model.selectedTarget)}
      options={model.options}
      onValueChange={handleChange}
      variant="compact"
    />
  );
}
