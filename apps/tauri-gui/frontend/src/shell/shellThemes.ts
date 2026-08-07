export const EDITORIAL_THEME = 'editorial' as const;
export const EDITORIAL_WINDOW_THEME = 'light' as const;

export type ShellTheme = typeof EDITORIAL_THEME;
export type ResolvedShellTheme = ShellTheme;
export type NativeShellTheme = typeof EDITORIAL_WINDOW_THEME;

export function isShellTheme(value: unknown): value is ShellTheme {
  return value === EDITORIAL_THEME;
}
