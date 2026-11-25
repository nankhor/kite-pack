import { useEffect, useState } from "react";
import type { Backend } from "../backend";

/** Asked when the window is closed while an install runs: closing stops the PS5's download. */
export function CloseQuestion({ backend }: { backend: Backend }) {
  const [open, setOpen] = useState(false);
  useEffect(
    () =>
      backend.onEvent((e) => {
        if (e.type === "closeAsked") setOpen(true);
      }),
    [backend],
  );
  if (!open) return null;
  return (
    <>
      <div className="close-backdrop" aria-hidden="true" />
      <div
        className="close-question"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="close-question-title"
        aria-describedby="close-question-text"
        onKeyDown={(e) => e.key === "Escape" && setOpen(false)}
      >
        <h2 id="close-question-title">An install is running</h2>
        <p id="close-question-text">
          The PS5 is downloading the package from this computer. If Kite Pack closes, the install stops until you open it
          again.
        </p>
        <div className="buttons">
          <button
            className="button secondary"
            onClick={() => {
              setOpen(false);
              void backend.quit();
            }}
          >
            Close anyway
          </button>
          <button className="button primary" autoFocus onClick={() => setOpen(false)}>
            Keep it open
          </button>
        </div>
      </div>
    </>
  );
}
