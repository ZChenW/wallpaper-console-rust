export function resolveImageBackendDisplay(
  raw: string | null,
  fulfilled: boolean,
): { display: string; shouldMigrate: boolean } {
  if (!fulfilled || raw === null) {
    return { display: '', shouldMigrate: false };
  }
  if (raw === 'swww') {
    return { display: 'awww', shouldMigrate: true };
  }
  if (raw === 'awww' || raw === 'mpvpaper') {
    return { display: raw, shouldMigrate: false };
  }
  return { display: 'awww', shouldMigrate: false };
}
