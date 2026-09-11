import { useEffect, useRef, useState } from 'react';

interface Props {
  readonly criteriaKey: string;
  readonly totalKnown: boolean;
  readonly total: number;
  readonly pending: boolean;
}

export function formatWallpaperCountAnnouncement(total: number): string {
  return `${total} ${total === 1 ? 'wallpaper' : 'wallpapers'} found`;
}

/** Decide whether this query visit should announce, ignoring pending/stale totals. */
export function shouldAnnounceLibraryTotal(input: {
  readonly criteriaKey: string;
  readonly totalKnown: boolean;
  readonly pending: boolean;
  readonly announcedCriteriaKey: string | null;
}): boolean {
  if (input.pending || !input.totalKnown) return false;
  return input.announcedCriteriaKey !== input.criteriaKey;
}

/** Announce the resolved count once per query visit, never per appended page. */
export default function LibraryResultAnnouncement({
  criteriaKey,
  totalKnown,
  total,
  pending,
}: Props) {
  const announcedCriteriaKey = useRef<string | null>(null);
  const [message, setMessage] = useState('');
  useEffect(() => {
    if (!shouldAnnounceLibraryTotal({
      criteriaKey,
      totalKnown,
      pending,
      announcedCriteriaKey: announcedCriteriaKey.current,
    })) {
      if (announcedCriteriaKey.current !== criteriaKey) setMessage('');
      return;
    }
    announcedCriteriaKey.current = criteriaKey;
    setMessage(formatWallpaperCountAnnouncement(total));
  }, [criteriaKey, pending, totalKnown, total]);
  return <span className="sr-only" role="status" aria-atomic="true">{message}</span>;
}
