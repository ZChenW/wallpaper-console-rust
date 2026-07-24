import { isShellTheme, resolveShellTheme, type ResolvedShellTheme, type ShellTheme } from './shellThemes.ts';

/**
 * localStorage key for the last shell theme preference.
 * Keep in sync with the inline bootstrap in `index.html`.
 */
export const SHELL_THEME_BOOTSTRAP_STORAGE_KEY = 'wc.shellThemePreference';

export function readBootstrappedShellThemePreference(
  storage: Pick<Storage, 'getItem'> | null | undefined = globalThis.localStorage,
): ShellTheme | null {
  if (!storage) return null;
  try {
    const raw = storage.getItem(SHELL_THEME_BOOTSTRAP_STORAGE_KEY);
    return isShellTheme(raw) ? raw : null;
  } catch {
    return null;
  }
}

export function writeBootstrappedShellThemePreference(
  theme: ShellTheme,
  storage: Pick<Storage, 'setItem'> | null | undefined = globalThis.localStorage,
): void {
  if (!storage) return;
  try {
    storage.setItem(SHELL_THEME_BOOTSTRAP_STORAGE_KEY, theme);
  } catch {
    // Private mode / quota — first paint may flash once next launch.
  }
}

export function resolveBootstrappedDocumentTheme(
  preference: ShellTheme | null,
  prefersDark: boolean,
): ResolvedShellTheme {
  return resolveShellTheme(preference ?? 'system', prefersDark);
}

/**
 * Apply a document theme before React preferences load.
 */
export function applyBootstrappedDocumentTheme(
  documentElement: Pick<HTMLElement, 'dataset'> = document.documentElement,
  storage: Pick<Storage, 'getItem'> | null | undefined = globalThis.localStorage,
  prefersDark: boolean = globalThis.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false,
): ResolvedShellTheme {
  const preference = readBootstrappedShellThemePreference(storage);
  const resolved = resolveBootstrappedDocumentTheme(preference, prefersDark);
  documentElement.dataset.theme = resolved;
  return resolved;
}
