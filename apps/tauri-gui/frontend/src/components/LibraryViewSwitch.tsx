import { memo } from 'react';

import type { LibraryViewMode } from '../shell/shellPreferences.ts';

export interface LibraryViewSwitchProps {
  readonly value: LibraryViewMode;
  readonly onChange: (mode: LibraryViewMode) => void;
  readonly disabled?: boolean;
}

export function LibraryViewSwitchView({
  value,
  onChange,
  disabled = false,
}: LibraryViewSwitchProps) {
  const button = (mode: LibraryViewMode, label: string) => (
    <button
      aria-pressed={value === mode}
      className="library-view-switch__button"
      disabled={disabled}
      key={mode}
      onClick={() => {
        if (mode !== value) onChange(mode);
      }}
      type="button"
    >
      {label}
    </button>
  );

  return (
    <div aria-label="Library view" className="library-view-switch" role="group">
      {button('grid', 'Grid')}
      {button('flow', 'Flow')}
    </div>
  );
}

export default memo(LibraryViewSwitchView);
