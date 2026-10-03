type Metric = { name: string; value: number; at: number; requestId?: string };

const metrics: Metric[] = [];
const MAX_METRICS = 200;

export function recordMetric(name: string, value: number, requestId?: string): void {
  metrics.push({ name, value, at: Date.now(), ...(requestId === undefined ? {} : { requestId }) });
  if (metrics.length > MAX_METRICS) metrics.splice(0, metrics.length - MAX_METRICS);
}

export async function measureAsync<T>(name: string, fn: () => Promise<T>): Promise<T> {
  const start = performance.now();
  try {
    return await fn();
  } finally {
    recordMetric(name, performance.now() - start);
  }
}

export function getRecentMetrics(): Metric[] {
  return [...metrics];
}

export function libraryMetricsEnabled(): boolean {
  if (import.meta.env?.DEV) return true;
  try { return localStorage.getItem('wc.debug.metrics') === 'on'; }
  catch { return false; }
}

// Injected before the page loads, only when the host has WCR_PERF enabled.
declare global {
  interface Window {
    __WCR_PERF__?: boolean;
    __WCR_GET_METRICS__?: typeof getRecentMetrics;
  }
}

export function applyTriggerTimestamp(): number | undefined {
  return typeof window !== 'undefined' && window.__WCR_PERF__
    ? performance.now() : undefined;
}

if (typeof window !== 'undefined' && window.__WCR_PERF__) {
  window.__WCR_GET_METRICS__ = getRecentMetrics;
}
