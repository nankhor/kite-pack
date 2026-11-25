import type { Phase } from "../state";

interface Props {
  phase: Phase;
  note: string;
  canShow: boolean;
  /** False while the picked DLC is still being read: packing then would leave them out. */
  canPack?: boolean;
  /** Send to PS5 is on: the button says so. */
  send?: boolean;
  /** A package: Install on PS5 is the main button. */
  install?: boolean;
  /** A package Forge can also pack: Pack stays, beside Install. */
  packable?: boolean;
  /** Convert: its button turns the dump into a .fpkg (and installs it when `send`). */
  convert?: boolean;
  onPack: () => void;
  onInstall?: () => void;
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
            {p.convert && (
              <button className="button primary big" onClick={p.onPack} disabled={p.canPack === false}>
                {p.send ? "Convert and install" : "Convert"}
              </button>
            )}
            {!p.convert && (!p.install || p.packable) && (
              <button
                className={p.install ? "button secondary" : "button primary big"}
                onClick={p.onPack}
                disabled={p.canPack === false}
              >
                {p.send ? "Pack and send" : "Pack"}
              </button>
            )}
            {p.install && (
              <button className="button primary big" onClick={p.onInstall}>
                Install on PS5
              </button>
            )}
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
              {p.convert ? "Convert another game" : p.install ? "Install another package" : "Pack another game"}
            </button>
          </>
        )}
      </div>
    </footer>
  );
}
