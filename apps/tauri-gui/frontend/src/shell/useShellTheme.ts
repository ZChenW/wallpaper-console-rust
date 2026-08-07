import { useLayoutEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { setTheme as setTauriTheme } from '@tauri-apps/api/app';

import {
  EDITORIAL_THEME,
  EDITORIAL_WINDOW_THEME,
  type NativeShellTheme,
  type ResolvedShellTheme,
} from './shellThemes.ts';

export interface ShellThemeSurface {
  setDocumentTheme(theme: ResolvedShellTheme): void;
  setWindowTheme(theme: NativeShellTheme): Promise<void>;
  revealWindow(): Promise<void>;
}

export async function applyShellThemeToSurface(
  surface: ShellThemeSurface,
): Promise<void> {
  surface.setDocumentTheme(EDITORIAL_THEME);
  try {
    await surface.setWindowTheme(EDITORIAL_WINDOW_THEME);
  } catch {
    // Browser, mock, and smoke environments may not expose a Tauri window.
  }
  try {
    await surface.revealWindow();
  } catch {
    // Browser, mock, and smoke environments may not expose a Tauri window.
  }
}

const browserThemeSurface: ShellThemeSurface = {
  setDocumentTheme(theme) {
    document.documentElement.dataset.theme = theme;
  },
  setWindowTheme(theme) {
    return setTauriTheme(theme);
  },
  // Prefer the Rust command (same path as single-instance focus). JS
  // window.show() was denied by capabilities and left the first launch hidden.
  revealWindow() {
    return invoke<void>('reveal_main_window');
  },
};

/** Keep the fixed Editorial document and native window themes in sync. */
export function useShellTheme(): void {
  const initializedRef = useRef(false);

  useLayoutEffect(() => {
    if (initializedRef.current) return;
    initializedRef.current = true;
    void applyShellThemeToSurface(browserThemeSurface);
  }, []);
}
