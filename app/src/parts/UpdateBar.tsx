import { useState } from "react";
import { formatSize } from "../model";
import { noteLines, percent, type UpdateView } from "../updates";

interface Props {
  view: UpdateView;
  /** A game is packing or sending: no restart until it's done. */
  busy: boolean;
  onUpdate: () => void;
  onLater: () => void;
  onRestart: () => void;
}

/** Under the tabs when there is a newer Kite Pack: what's new, the download, then the restart. */
export function UpdateBar({ view, busy, onUpdate, onLater, onRestart }: Props) {
  const [notes, setNotes] = useState(false);
  if (view.state === "none") return null;
  const { info } = view;
  const lines = noteLines(info.notes);

  return (
    <section className="update-bar" aria-label="Update">
      <div className="update-row">
        {view.state === "available" && (
          <>
            <p className="update-text">
              <strong>Kite Pack {info.version} is out.</strong>
              {info.size > 0 && <span className="update-size"> {formatSize(info.size)}</span>}
            </p>
            {lines.length > 0 && (
              <button className="small-button" aria-expanded={notes} onClick={() => setNotes(!notes)}>
                What's new
              </button>
            )}
            <button className="small-button" onClick={onLater}>
              Later
            </button>
            <button className="small-button primary" onClick={onUpdate}>
              Update
            </button>
          </>
        )}
        {view.state === "downloading" && (
          <>
            <p className="update-text" role="status">
              <strong>Downloading Kite Pack {info.version}…</strong> {percent(view.done, view.total)}
            </p>
            <div
              className="update-progress"
              role="progressbar"
              aria-label={`Downloading Kite Pack ${info.version}`}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={view.total > 0 ? Math.floor((view.done / view.total) * 100) : 0}
            >
              <div className="update-fill" style={{ width: percent(view.done, view.total) || "0%" }} />
            </div>
          </>
        )}
        {view.state === "ready" && (
          <>
            <p className="update-text" role="status">
              <strong>Kite Pack {info.version} is installed.</strong>{" "}
              {busy ? "Restart once this game is done to use it." : "Restart to use it."}
            </p>
            <button className="small-button" onClick={onLater}>
              Later
            </button>
            <button className="small-button primary" disabled={busy} onClick={onRestart}>
              Restart now
            </button>
          </>
        )}
        {view.state === "failed" && (
          <>
            <p className="update-text" role="alert">
              <strong>Kite Pack {info.version} didn't install.</strong> {view.reason}
            </p>
            <button className="small-button" onClick={onLater}>
              Later
            </button>
            <button className="small-button primary" onClick={onUpdate}>
              Try again
            </button>
          </>
        )}
      </div>
      {notes && view.state === "available" && (
        <ul className="update-notes" aria-label="What's new">
          {lines.map((l, i) => (
            <li key={i}>{l}</li>
          ))}
        </ul>
      )}
    </section>
  );
}
