type Metric = { name: string; value: number; at: number };

const metrics: Metric[] = [];
const MAX_METRICS = 200;

export function recordMetric(name: string, value: number): void {
  metrics.push({ name, value, at: Date.now() });
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
