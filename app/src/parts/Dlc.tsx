import { useState } from "react";
import { formatSize } from "../model";
import type { PickedDlc } from "../types";

interface Props {
  picked: PickedDlc[];
  /** Why DLC can't be added now (Send is off, say); null when it can. */
  blocked: string | null;
  /** Checking picked packages. */
  busy: boolean;
  /** Stands in for a DLC that has no icon of its own. */
  gameIcon: string | null;
  onAddFiles: () => void;
  onAddFolder: () => void;
  onRemove: (path: string) => void;
  onRemoveAll: () => void;
}

const X = (
  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" aria-hidden="true">
    <path d="M6 6l12 12M18 6L6 18" />
  </svg>
);

const PLUS = (
  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" aria-hidden="true">
    <path d="M12 5v14M5 12h14" />
  </svg>
);

const name = (d: PickedDlc) => d.info.name ?? d.info.label;

function Icon({ dlc, gameIcon }: { dlc: PickedDlc; gameIcon: string | null }) {
  if (dlc.info.iconDataUrl) return <img className="dlc-icon" src={dlc.info.iconDataUrl} alt="" />;
  if (gameIcon) return <img className="dlc-icon stand-in" src={gameIcon} alt="" />;
  return <span className="dlc-icon blank" aria-hidden="true" />;
}

/** DLC to install on the PS5 after the game: one line that opens the full list. */
export function Dlc({ picked, blocked, busy, gameIcon, onAddFiles, onAddFolder, onRemove, onRemoveAll }: Props) {
  const [open, setOpen] = useState(false);
  const shown = open && picked.length > 0;
  const total = formatSize(picked.reduce((sum, d) => sum + d.info.bytes, 0));

  let hint = "Add .pkg files, or a folder of them. The PS5 installs each with its own installer, after the game.";
  if (blocked) hint = blocked;
  else if (busy) hint = "Checking the packages…";
  else if (picked.length > 0) hint = "Installed one by one on the PS5, with its own installer, after the game.";

  return (
    <section className={blocked ? "dlc blocked" : "dlc"} aria-label="DLC">
      <div className="dlc-head">
        <h2>DLC</h2>
        <span className="dlc-hint">{hint}</span>
      </div>
      <div className="dlc-controls">
        {picked.length > 0 && (
          <button
            className="dlc-summary"
            aria-expanded={shown}
            aria-controls="dlc-list"
            onClick={() => setOpen(!open)}
          >
            <span className="dlc-count">{picked.length} DLC</span>
            <span className="dlc-total">{total}</span>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d={shown ? "M6 15l6-6 6 6" : "M6 9l6 6 6-6"} />
            </svg>
          </button>
        )}
        <button className="add-button" onClick={onAddFiles} disabled={!!blocked || busy}>
          {PLUS}Add packages
        </button>
        <button className="add-button" onClick={onAddFolder} disabled={!!blocked || busy}>
          {PLUS}Add a folder
        </button>
        {shown && (
          <div
            id="dlc-list"
            className="popover"
            role="region"
            aria-label="DLC to install"
            onKeyDown={(e) => e.key === "Escape" && setOpen(false)}
          >
            <div className="popover-head">
              <span className="popover-title">
                {picked.length} DLC <span className="popover-total">· {total}</span>
              </span>
              <button
                className="small-button"
                onClick={() => {
                  onRemoveAll();
                  setOpen(false);
                }}
              >
                Remove all
              </button>
              <button className="round-button" aria-label="Close the list" onClick={() => setOpen(false)}>
                {X}
              </button>
            </div>
            <ol className="popover-list">
              {picked.map((d, i) => (
                <li key={d.path}>
                  <span className="popover-n">{i + 1}</span>
                  <Icon dlc={d} gameIcon={gameIcon} />
                  <span className="popover-name">{name(d)}</span>
                  <span className="popover-size">{formatSize(d.info.bytes)}</span>
                  <button className="round-button" aria-label={`Remove ${name(d)}`} onClick={() => onRemove(d.path)}>
                    {X}
                  </button>
                </li>
              ))}
            </ol>
            <p className="popover-note">Installed in this order, after the game.</p>
          </div>
        )}
      </div>
    </section>
  );
}
