import { useEffect, useRef, useState } from "react";
import type { BarPart, DlcItem, JobBar as Bar } from "../model";

function PartIcon({ state }: { state: BarPart["state"] }) {
  switch (state) {
    case "done":
      return (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <circle cx="12" cy="12" r="11" fill="var(--mint)" />
          <path d="M7 12.5l3.2 3.2L17 9" stroke="#0a0d10" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      );
    case "active":
      return (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <circle cx="12" cy="12" r="10" stroke="#ffffff" strokeWidth="2.4" />
          <circle cx="12" cy="12" r="5" fill="#ffffff" />
        </svg>
      );
    case "paused":
      return (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <circle cx="12" cy="12" r="10" stroke="var(--amber)" strokeWidth="2.4" />
          <path d="M10 8.5v7M14 8.5v7" stroke="var(--amber)" strokeWidth="2.6" strokeLinecap="round" />
        </svg>
      );
    case "failed":
      return (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <circle cx="12" cy="12" r="10" stroke="var(--fail)" strokeWidth="2.4" />
          <path d="M12 7.5v6M12 16.5v.5" stroke="var(--fail)" strokeWidth="2.6" strokeLinecap="round" />
        </svg>
      );
    default:
      return (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <circle cx="12" cy="12" r="10" stroke="rgba(255,255,255,0.4)" strokeWidth="2.2" strokeDasharray="3 3" />
        </svg>
      );
  }
}

const ITEM_WORDS: Record<DlcItem["state"], string> = {
  waiting: "Waiting",
  installing: "Installing",
  installed: "Installed",
  failed: "Failed",
};

/** While a job runs: one bar for the whole job, in parts, and what is happening now above it. */
export function JobBar({ bar }: { bar: Bar }) {
  const [open, setOpen] = useState(false);
  const list = useRef<HTMLOListElement>(null);
  const dlc = bar.parts.find((p) => (p.items?.length ?? 0) > 1);

  // The list opens on the DLC installing now, not on the first ones, long done.
  useEffect(() => {
    const ol = list.current;
    const now = ol?.querySelector<HTMLElement>(".item-installing, .item-failed");
    if (ol && now) ol.scrollTop = now.offsetTop - ol.offsetTop - (ol.clientHeight - now.offsetHeight) / 2;
  }, [open]);
  return (
    <section className="job" aria-label="Progress">
      <div className="job-head">
        <span className={`job-title ${bar.tone}`}>{bar.head}</span>
        <span className="job-detail">{bar.detail}</span>
      </div>
      <div
        className="job-bar"
        role="progressbar"
        aria-label={bar.head}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={bar.overall}
        aria-valuetext={bar.detail || undefined}
      >
        {bar.parts.map((p) => (
          <div key={p.label} className={`job-part ${p.state}`} style={{ flexGrow: p.weight }}>
            {/* Pack and send: what is packed, lighter, under what the PS5 has. */}
            {p.under !== undefined && p.state !== "done" && p.state !== "failed" && (
              <div className="job-under" style={{ width: `${p.under}%` }} />
            )}
            <div className={p.fill === null ? "job-fill moving" : "job-fill"} style={{ width: `${p.fill ?? 100}%` }} />
          </div>
        ))}
      </div>
      <ol className="job-steps" aria-label="Steps">
        {bar.parts.map((p) => (
          <li key={p.label} className={`job-step ${p.state}`} style={{ flexGrow: p.weight }}>
            <span className="job-step-label" title={p.label}>
              <PartIcon state={p.state} />
              <span className="job-step-name">{p.label}</span>
            </span>
            {p.note && (
              <span className="job-step-note" title={p.note}>
                {p.note}
              </span>
            )}
          </li>
        ))}
      </ol>
      {dlc && (
        <div className="job-more">
          <button className="small-button" aria-expanded={open} aria-controls="dlc-states" onClick={() => setOpen(!open)}>
            {open ? "Hide the DLC list" : `See all ${dlc.items!.length} DLC`}
          </button>
          {open && <div className="sheet-backdrop" aria-hidden="true" onMouseDown={() => setOpen(false)} />}
          {open && (
            <div
              id="dlc-states"
              className="popover step-popover sheet"
              role="dialog"
              aria-modal="true"
              aria-label="Each DLC"
              onKeyDown={(e) => e.key === "Escape" && setOpen(false)}
            >
              <div className="popover-head">
                <span className="popover-title">
                  {dlc.items!.length} DLC
                  <span className="popover-total">
                    {" "}
                    · {dlc.items!.filter((i) => i.state === "installed").length} installed
                  </span>
                </span>
                <button className="round-button" aria-label="Close the list" autoFocus onClick={() => setOpen(false)}>
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" aria-hidden="true">
                    <path d="M6 6l12 12M18 6L6 18" />
                  </svg>
                </button>
              </div>
              <ol className="popover-list" ref={list}>
                {dlc.items!.map((item, i) => (
                  <li key={i} className={`item-${item.state}`}>
                    <span className="popover-n">{i + 1}</span>
                    <span className="popover-name">{item.name}</span>
                    <span className="popover-state" title={item.note ?? undefined}>
                      {ITEM_WORDS[item.state]}
                      {item.note ? `: ${item.note}` : ""}
                    </span>
                  </li>
                ))}
              </ol>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
