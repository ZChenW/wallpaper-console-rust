import { useCallback, useRef, useState } from 'react';
import { commandErrorFeedback, type CommandFeedback } from '../api/feedback.ts';
import type { MpvpaperReapplyResultDTO } from '../api/types.ts';

interface Options {
  saveOptions(options: string): Promise<void>;
  reapply(): Promise<MpvpaperReapplyResultDTO>;
  refreshCurrent(): Promise<void>;
  wallpaperApplying: boolean;
  setFeedback(feedback: CommandFeedback): void;
}

export function useMpvpaperReapply(options: Options) {
  const [applying, setApplying] = useState(false);
  const busy = useRef(false);
  const latest = useRef(options);
  latest.current = options;

  const apply = useCallback(async (argumentsValue: string) => {
    if (busy.current || latest.current.wallpaperApplying) return;
    busy.current = true;
    setApplying(true);
    const { saveOptions, reapply, refreshCurrent, setFeedback } = latest.current;
    let saved = false;
    try {
      await saveOptions(argumentsValue);
      saved = true;
      const result = await reapply();
      if (result.failures.length > 0) {
        setFeedback({
          state: 'error', label: 'Arguments saved; reapply failed',
          detail: result.failures.map(({ output, message }) => `${output}: ${message}`).join('\n'),
        });
      } else {
        setFeedback({
          state: 'success',
          label: result.appliedOutputs.length > 0 ? 'mpv arguments applied' : 'mpv arguments saved',
          detail: result.appliedOutputs.length > 0 ? result.appliedOutputs.join(', ') : 'No confirmed mpvpaper wallpapers to reload.',
        });
      }
    } catch (error) {
      setFeedback(commandErrorFeedback(saved ? 'Arguments saved; reapply' : 'Save mpv arguments', error));
    } finally {
      try {
        if (saved) await refreshCurrent();
      } catch (error) {
        setFeedback(commandErrorFeedback('Refresh wallpaper status', error));
      } finally {
        busy.current = false;
        setApplying(false);
      }
    }
  }, []);

  return { applying, apply };
}
