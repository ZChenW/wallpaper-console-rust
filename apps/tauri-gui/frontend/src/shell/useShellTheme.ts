import { useEffect, useLayoutEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { setTheme as setTauriTheme } from '@tauri-apps/api/app';

import type { ShellTheme } from './shellPreferences.ts';
import {
  readBootstrappedShellThemePreference,
  writeBootstrappedShellThemePreference,
} from './shellThemeBootstrap.ts';
import {
  nativeWindowTheme,
  resolveShellTheme,
  type NativeShellTheme,
  type ResolvedShellTheme,
} from './shellThemes.ts';

export type { ResolvedShellTheme } from './shellThemes.ts';
export { resolveShellTheme } from './shellThemes.ts';

export interface ShellThemeSurface {
  setDocumentTheme(theme: ResolvedShellTheme): void;
  setWindowTheme(theme: NativeShellTheme | null): Promise<void>;
  revealWindow(): Promise<void>;
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

/**
 * Keep the last known preference for the next cold start's inline bootstrap.
 * While preferences are still loading, leave any bootstrapped `data-theme`
 * alone so the first visible paint is the previous session's theme — not a
 * blank frame or the compile-time default.
 */
export function useShellTheme(theme: ShellTheme, ready = true): void {
  const revealedRef = useRef(false);

  useLayoutEffect(() => {
    if (!ready) {
      // Preserve index.html / prior-session bootstrap. Clearing here is what
      // produced: blank window → default tokens → saved theme.
      return undefined;
    }
    const media = window.matchMedia?.('(prefers-color-scheme: dark)');
    const apply = () => {
      const resolved = resolveShellTheme(theme, media?.matches ?? false);
      browserThemeSurface.setDocumentTheme(resolved);
      writeBootstrappedShellThemePreference(theme);
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

  useLayoutEffect(() => {
    if (revealedRef.current) return;
    const hasDocumentTheme = Boolean(document.documentElement.dataset.theme);
    if (!hasDocumentTheme) return;
    // With a prior-session bootstrap, reveal immediately. Otherwise wait for
    // preferences so the first mapped frame is the authoritative theme.
    const hasBootstrap = readBootstrappedShellThemePreference() !== null;
    if (!ready && !hasBootstrap) return;
    revealedRef.current = true;
    void browserThemeSurface.revealWindow().catch(() => {
      // Allow a later ready/theme commit to retry (e.g. invoke not ready yet).
      revealedRef.current = false;
    });
  }, [ready, theme]);
}
