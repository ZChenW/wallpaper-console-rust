import { useEffect, useState } from 'react';
import { getRecentMetrics } from '../perf/metrics';
import { useThumbnailStore } from '../state/ThumbnailStoreContext';

export default function PerformanceOverlay() {
  const [visible, setVisible] = useState(false);
  const [, force] = useState(0);
  const thumbnailStats = useThumbnailStore().stats();

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 'p') {
        setVisible((v) => !v);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  useEffect(() => {
    if (!visible) return;
    const timer = window.setInterval(() => force((v) => v + 1), 1000);
    return () => window.clearInterval(timer);
  }, [visible]);

  if (!visible) return null;
  const recent = getRecentMetrics().slice(-8).reverse();
  return (
    <div className="perf-overlay">
      <div>thumbnail.pending: {thumbnailStats.pending}</div>
      <div>thumbnail.active: {thumbnailStats.active}</div>
      <div>thumbnail.cached: {thumbnailStats.cached}</div>
      <div>thumbnail.failures: {thumbnailStats.failures}</div>
      {recent.map((m, i) => (
        <div key={`${m.name}-${m.at}-${i}`}>{m.name}: {m.value.toFixed(1)}</div>
      ))}
    </div>
  );
}
