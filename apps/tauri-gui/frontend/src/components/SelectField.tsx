import { Check, ChevronDown, ChevronUp } from 'lucide-react';
import { Select as RadixSelect } from 'radix-ui';

export type SelectFieldOption = {
  readonly value: string;
  readonly label: string;
  readonly disabled?: boolean;
};

export interface SelectFieldProps {
  readonly 'aria-label': string;
  readonly value: string;
  readonly options: readonly SelectFieldOption[];
  readonly onValueChange: (value: string) => void;
  readonly disabled?: boolean;
  readonly variant?: 'compact' | 'settings';
  readonly className?: string;
  readonly dataBehaviorControl?: boolean;
}

export default function SelectField({
  'aria-label': ariaLabel,
  value,
  options,
  onValueChange,
  disabled = false,
  variant = 'compact',
  className,
  dataBehaviorControl = false,
}: SelectFieldProps) {
  const triggerClassName = [
    'select-field-trigger',
    `select-field-trigger--${variant}`,
    className,
  ].filter(Boolean).join(' ');

  return (
    <RadixSelect.Root value={value} disabled={disabled} onValueChange={onValueChange}>
      <RadixSelect.Trigger
        aria-label={ariaLabel}
        className={triggerClassName}
        data-behavior-control={dataBehaviorControl ? true : undefined}
        data-value={value}
      >
        <RadixSelect.Value />
        <RadixSelect.Icon className="select-field-trigger__icon">
          <ChevronDown aria-hidden="true" size={14} />
        </RadixSelect.Icon>
      </RadixSelect.Trigger>
      <RadixSelect.Portal>
        <RadixSelect.Content
          className="select-field-content"
          collisionPadding={8}
          position="popper"
          sideOffset={5}
        >
          <RadixSelect.ScrollUpButton className="select-field-scroll-button">
            <ChevronUp aria-hidden="true" size={14} />
          </RadixSelect.ScrollUpButton>
          <RadixSelect.Viewport className="select-field-viewport">
            {options.map((option) => (
              <RadixSelect.Item
                className="select-field-item"
                disabled={option.disabled}
                key={option.value}
                value={option.value}
              >
                <RadixSelect.ItemText>{option.label}</RadixSelect.ItemText>
                <RadixSelect.ItemIndicator className="select-field-item__indicator">
                  <Check aria-hidden="true" size={14} strokeWidth={2.4} />
                </RadixSelect.ItemIndicator>
              </RadixSelect.Item>
            ))}
          </RadixSelect.Viewport>
          <RadixSelect.ScrollDownButton className="select-field-scroll-button">
            <ChevronDown aria-hidden="true" size={14} />
          </RadixSelect.ScrollDownButton>
        </RadixSelect.Content>
      </RadixSelect.Portal>
    </RadixSelect.Root>
  );
}
