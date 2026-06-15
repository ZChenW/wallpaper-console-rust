export function nextSelectionForClick(
  paths: string[],
  selectedPaths: Set<string>,
  lastClickedPath: string | null,
  clickedPath: string,
  event: { ctrlKey?: boolean; metaKey?: boolean; shiftKey?: boolean },
  indexByPath: Map<string, number>,
): { selectedPaths: Set<string>; lastClickedPath: string | null } {
  const next = new Set(selectedPaths);

  if (event.ctrlKey || event.metaKey) {
    if (next.has(clickedPath)) next.delete(clickedPath);
    else next.add(clickedPath);
    return { selectedPaths: next, lastClickedPath: clickedPath };
  }
  if (event.shiftKey && lastClickedPath) {
    const current = indexByPath.get(clickedPath);
    const previous = indexByPath.get(lastClickedPath);
    if (current !== undefined && previous !== undefined) {
      const start = Math.min(current, previous);
      const end = Math.max(current, previous);
      for (let i = start; i <= end; i += 1) {
        next.add(paths[i]);
      }
      return { selectedPaths: next, lastClickedPath };
    }
  }
  return { selectedPaths: new Set([clickedPath]), lastClickedPath: clickedPath };
}

import { useCallback, useMemo, useRef } from 'react';

interface GridSelectionOptions {
  paths: string[];
  selectedPaths?: Set<string>;
  onSelectionChange?: (paths: Set<string>) => void;
}

export function useGridSelection({ paths, selectedPaths, onSelectionChange }: GridSelectionOptions) {
  const lastClickedRef = useRef<string | null>(null);
  const indexByPath = useMemo(() => new Map(paths.map((path, index) => [path, index])), [paths]);

  const clearSelection = useCallback(() => {
    onSelectionChange?.(new Set());
  }, [onSelectionChange]);

  const handleClick = useCallback((event: { ctrlKey?: boolean; metaKey?: boolean; shiftKey?: boolean }, path: string) => {
    if (!onSelectionChange) return;
    const result = nextSelectionForClick(paths, selectedPaths ?? new Set(), lastClickedRef.current, path, event, indexByPath);
    onSelectionChange(result.selectedPaths);
    lastClickedRef.current = result.lastClickedPath;
  }, [paths, selectedPaths, onSelectionChange, indexByPath]);

  return { clearSelection, handleClick };
}
