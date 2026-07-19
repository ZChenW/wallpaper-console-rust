import { useEffect, useLayoutEffect } from 'react';
import { setTheme as setTauriTheme } from '@tauri-apps/api/app';

import type { ShellTheme } from './shellPreferences.ts';
import {
  nativeWindowTheme,
  type NativeShellTheme,
  type ResolvedShellTheme,
} from './shellThemes.ts';

export type { ResolvedShellTheme } from './shellThemes.ts';

export interface ShellThemeSurface {
  setDocumentTheme(theme: ResolvedShellTheme): void;
  setWindowTheme(theme: NativeShellTheme | null): Promise<void>;
}

export function resolveShellTheme(
  theme: ShellTheme,
  prefersDark: boolean,
): ResolvedShellTheme {
  return theme === 'system' ? (prefersDark ? 'dark' : 'light') : theme;
}

export function resolvedThemeWhenReady(
  theme: ShellTheme,
  prefersDark: boolean,
  ready: boolean,
): ResolvedShellTheme | null {
  return ready ? resolveShellTheme(theme, prefersDark) : null;
}

export async function applyShellThemeToSurface(
  theme: ShellTheme,
  prefersDark: boolean,
  surface: ShellThemeSurface,
): Promise<void> {
  surface.setDocumentTheme(resolveShellTheme(theme, prefersDark));
  try {
    await surface.setWindowTheme(nativeWindowTheme(theme));
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

export function useShellTheme(theme: ShellTheme, ready = true): void {
  useLayoutEffect(() => {
    if (!ready) {
      delete document.documentElement.dataset.theme;
      return undefined;
    }
    const media = window.matchMedia?.('(prefers-color-scheme: dark)');
    const apply = () => {
      browserThemeSurface.setDocumentTheme(resolveShellTheme(theme, media?.matches ?? false));
    };
    apply();
    if (theme !== 'system' || !media) return undefined;
    media.addEventListener('change', apply);
    return () => media.removeEventListener('change', apply);
  }, [ready, theme]);

  useEffect(() => {
    if (!ready) return;
    void browserThemeSurface.setWindowTheme(nativeWindowTheme(theme)).catch(() => {
      // Browser, mock, and smoke environments may not expose a Tauri window.
    });
  }, [ready, theme]);
}
