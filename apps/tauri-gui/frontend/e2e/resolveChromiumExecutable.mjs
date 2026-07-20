import { accessSync, constants } from 'node:fs';

const SYSTEM_CANDIDATES = [
  process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH,
  process.env.CHROMIUM_PATH,
  '/usr/bin/chromium',
  '/usr/bin/chromium-browser',
  '/usr/bin/google-chrome-stable',
  '/usr/bin/google-chrome',
  '/opt/google/chrome/chrome',
].filter((value) => typeof value === 'string' && value.trim().length > 0);

/**
 * Prefer an already-installed system Chromium/Chrome binary.
 * Never downloads browsers.
 */
export function resolveSystemChromiumExecutable() {
  const explicitPath = process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH?.trim();
  if (explicitPath) {
    try {
      accessSync(explicitPath, constants.X_OK);
      return explicitPath;
    } catch (error) {
      throw new Error(
        `PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH is set but not executable: ${explicitPath} (${error instanceof Error ? error.message : String(error)})`,
      );
    }
  }

  for (const candidate of SYSTEM_CANDIDATES) {
    try {
      accessSync(candidate, constants.X_OK);
      return candidate;
    } catch {
      // try next candidate
    }
  }
  return null;
}
