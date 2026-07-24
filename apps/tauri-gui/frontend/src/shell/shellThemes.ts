export const SHELL_THEME_OPTIONS = Object.freeze([
  { value: 'system', label: 'System' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
  { value: 'glass', label: 'Glass' },
  { value: 'editorial', label: 'Editorial' },
] as const);

export type ShellTheme = (typeof SHELL_THEME_OPTIONS)[number]['value'];
export type ResolvedShellTheme = Exclude<ShellTheme, 'system'>;
export type NativeShellTheme = 'light' | 'dark';

const SHELL_THEME_VALUES: ReadonlySet<string> = new Set(
  SHELL_THEME_OPTIONS.map(({ value }) => value),
);

export function isShellTheme(value: unknown): value is ShellTheme {
  return typeof value === 'string' && SHELL_THEME_VALUES.has(value);
}

export function resolveShellTheme(
  theme: ShellTheme,
  prefersDark: boolean,
): ResolvedShellTheme {
  return theme === 'system' ? (prefersDark ? 'dark' : 'light') : theme;
}

export function nativeWindowTheme(theme: ShellTheme): NativeShellTheme | null {
  if (theme === 'system') return null;
  if (theme === 'glass') return 'dark';
  if (theme === 'editorial') return 'light';
  return theme;
}
