import { useEffect, useState } from 'react';
import { getRecentMetrics } from '../perf/metrics';

export default function PerformanceOverlay() {
  const [visible, setVisible] = useState(false);
  const [, force] = useState(0);

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
      {recent.map((m, i) => (
        <div key={`${m.name}-${m.at}-${i}`}>{m.name}: {m.value.toFixed(1)}</div>
      ))}
    </div>
  );
}
