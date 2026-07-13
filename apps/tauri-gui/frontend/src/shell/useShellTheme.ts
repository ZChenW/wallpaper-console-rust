import { useEffect } from 'react';
import { setTheme as setTauriTheme } from '@tauri-apps/api/app';

import type { ShellTheme } from './shellPreferences.ts';

export type ResolvedShellTheme = 'light' | 'dark';

export interface ShellThemeSurface {
  setDocumentTheme(theme: ResolvedShellTheme): void;
  setWindowTheme(theme: ResolvedShellTheme | null): Promise<void>;
}

export function resolveShellTheme(
  theme: ShellTheme,
  prefersDark: boolean,
): ResolvedShellTheme {
  return theme === 'system' ? (prefersDark ? 'dark' : 'light') : theme;
}

export async function applyShellThemeToSurface(
  theme: ShellTheme,
  prefersDark: boolean,
  surface: ShellThemeSurface,
): Promise<void> {
  surface.setDocumentTheme(resolveShellTheme(theme, prefersDark));
  try {
    await surface.setWindowTheme(theme === 'system' ? null : theme);
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
};

export function useShellTheme(theme: ShellTheme): void {
  useEffect(() => {
    const media = window.matchMedia?.('(prefers-color-scheme: dark)');
    const apply = () => {
      void applyShellThemeToSurface(theme, media?.matches ?? false, browserThemeSurface);
    };
    apply();
    if (theme !== 'system' || !media) return undefined;
    media.addEventListener('change', apply);
    return () => media.removeEventListener('change', apply);
  }, [theme]);
}
