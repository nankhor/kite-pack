import type { Phase } from "../state";

interface Props {
  phase: Phase;
  note: string;
  canShow: boolean;
  onPack: () => void;
  onChangeGame: () => void;
  onCancel: () => void;
  onRetry: () => void;
  onShow: () => void;
  onAnother: () => void;
}

/** The line about what happens, and the buttons for this state. */
export function Footer(p: Props) {
  return (
    <footer className="footer">
      <p className="note" aria-live="polite">
        {p.note}
      </p>
      <div className="buttons">
        {p.phase === "ready" && (
          <>
            <button className="button secondary" onClick={p.onChangeGame}>
              Change game
            </button>
            <button className="button primary big" onClick={p.onPack}>
              Pack
            </button>
          </>
        )}
        {p.phase === "running" && (
          <button className="button secondary" onClick={p.onCancel}>
            Cancel
          </button>
        )}
        {p.phase === "failed" && (
          <>
            <button className="button secondary" onClick={p.onCancel}>
              Cancel
            </button>
            <button className="button primary" onClick={p.onRetry}>
              Retry
            </button>
          </>
        )}
        {p.phase === "done" && (
          <>
            {p.canShow && (
              <button className="button secondary" onClick={p.onShow}>
                Show folder
              </button>
            )}
            <button className="button primary" onClick={p.onAnother}>
              Pack another game
            </button>
          </>
        )}
      </div>
    </footer>
  );
}
