import { useMemo, type KeyboardEvent } from 'react';

import { committedNumberDraft } from './deferredNumberInput.ts';

export interface DeferredNumberInputProps {
  readonly 'aria-label': string;
  readonly confirmed: number;
  readonly disabled?: boolean;
  readonly max?: number;
  readonly min?: number;
  readonly step?: number | string;
  readonly onCommit: (value: number) => void;
  readonly unit?: string;
  readonly unitKind?: string;
}

export function DeferredNumberInput({
  confirmed,
  disabled = false,
  max,
  min,
  step,
  onCommit,
  unit,
  unitKind,
  'aria-label': ariaLabel,
}: DeferredNumberInputProps) {
  const inputKey = useMemo(() => String(confirmed), [confirmed]);

  const commitFromInput = (input: HTMLInputElement) => {
    const next = committedNumberDraft(input.value, confirmed);
    input.value = String(confirmed);
    if (next !== confirmed) onCommit(next);
  };

  const handleBlur = (event: { currentTarget: HTMLInputElement }) => {
    commitFromInput(event.currentTarget);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      event.preventDefault();
      event.currentTarget.blur();
      return;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      event.currentTarget.value = String(confirmed);
      event.currentTarget.blur();
    }
  };

  return (
    <span className="settings-number-control">
      <input
        aria-label={unit ? `${ariaLabel} (${unit})` : ariaLabel}
        data-behavior-control={true}
        defaultValue={String(confirmed)}
        disabled={disabled}
        key={inputKey}
        max={max}
        min={min}
        onBlur={handleBlur}
        onKeyDown={handleKeyDown}
        step={step}
        type="number"
      />
      {unit ? (
        <span aria-hidden="true" data-control-unit={unitKind ?? 'number'}>{unit}</span>
      ) : null}
    </span>
  );
}
