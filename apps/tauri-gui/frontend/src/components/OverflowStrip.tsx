import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type HTMLAttributes,
} from 'react';

import {
  overflowStripState,
  overflowStripVerticalWheelDelta,
  type OverflowStripState,
} from './overflowStrip.ts';

export default function OverflowStrip({
  onScroll,
  ...props
}: HTMLAttributes<HTMLDivElement>) {
  const elementRef = useRef<HTMLDivElement | null>(null);
  const [overflow, setOverflow] = useState<OverflowStripState>('none');
  const updateOverflow = useCallback((element: HTMLDivElement | null) => {
    if (!element) return;
    setOverflow(overflowStripState(
      element.scrollLeft,
      element.clientWidth,
      element.scrollWidth,
    ));
  }, []);
  const setElement = useCallback((element: HTMLDivElement | null) => {
    elementRef.current = element;
    updateOverflow(element);
  }, [updateOverflow]);

  useEffect(() => {
    const element = elementRef.current;
    if (!element) return undefined;
    updateOverflow(element);
    const observer = new ResizeObserver(() => updateOverflow(element));
    observer.observe(element);
    return () => observer.disconnect();
  }, [updateOverflow]);

  useEffect(() => {
    updateOverflow(elementRef.current);
  });

  useEffect(() => {
    const element = elementRef.current;
    if (!element) return undefined;
    const handleWheel = (event: WheelEvent) => {
      if (event.ctrlKey || element.scrollWidth <= element.clientWidth) return;
      const travel = overflowStripVerticalWheelDelta(
        event.deltaX,
        event.deltaY,
        event.deltaMode,
        element.clientWidth,
      );
      if (travel === 0) return;
      const previousScrollLeft = element.scrollLeft;
      element.scrollLeft += travel;
      if (element.scrollLeft !== previousScrollLeft) {
        event.preventDefault();
      }
    };
    element.addEventListener('wheel', handleWheel, { passive: false });
    return () => {
      element.removeEventListener('wheel', handleWheel);
    };
  }, []);

  return (
    <div
      {...props}
      data-overflow={overflow}
      onScroll={(event) => {
        updateOverflow(event.currentTarget);
        onScroll?.(event);
      }}
      ref={setElement}
    />
  );
}
