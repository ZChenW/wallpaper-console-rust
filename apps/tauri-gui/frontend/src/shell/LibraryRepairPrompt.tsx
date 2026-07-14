import type { LibraryRepairFault } from './libraryRepair.ts';

export interface LibraryRepairPromptProps {
  readonly fault: LibraryRepairFault | null;
  readonly pending: boolean;
  readonly onRepair: () => void;
}

export default function LibraryRepairPrompt({
  fault,
  pending,
  onRepair,
}: LibraryRepairPromptProps) {
  if (!fault) return null;

  return (
    <section aria-label="Library repair" className="library-repair-prompt" role="alert">
      <strong>{fault.message}</strong>
      <p>Wallpaper files will not be deleted. Repair rebuilds the library index.</p>
      <div className="library-repair-prompt__actions">
        <button
          className="btn"
          data-library-repair={true}
          disabled={pending}
          onClick={onRepair}
          type="button"
        >
          {pending ? 'Repairing…' : 'Repair library'}
        </button>
        <details>
          <summary>Technical details</summary>
          <pre>{fault.technicalDetails}</pre>
        </details>
      </div>
    </section>
  );
}
