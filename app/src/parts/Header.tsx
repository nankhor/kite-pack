import type { KeyboardEvent } from "react";
import type { Badge } from "../model";
import type { Status, View } from "../types";

interface Props {
  view: View;
  status: Status | null;
  /** How the job on the Pack tab stands, seen from every tab. */
  badge: Badge | null;
  onView: (view: View) => void;
}

/** The chip's dot colour and words for each PS5 state. */
function chip(status: Status | null): [string, string] {
  switch (status?.state) {
    case "running":
      return ["mint", "Helper running"];
    case "notReachable":
      return ["amber", "Not reachable"];
    case "refused":
      return ["amber", "Can't connect"];
    case "notSetUp":
      return ["grey", "Not set up"];
    default:
      return ["grey", "Checking"];
  }
}

const TABS: [View, string][] = [
  ["pack", "Pack"],
  ["ps5", "PS5"],
  ["history", "History"],
];

/** The tab id, for the panel to name itself by. */
export const tabId = (view: View) => `tab-${view}`;

/** App name and the tabs on the left; the PS5 chip (opens the PS5 tab) and Settings on the right. */
export function Header({ view, status, badge, onView }: Props) {
  const [dot, words] = chip(status);

  // Left and right move between the tabs, as in any tab bar.
  function onKey(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const at = Math.max(0, TABS.findIndex(([v]) => v === view));
    const next = TABS[(at + (e.key === "ArrowRight" ? 1 : TABS.length - 1)) % TABS.length][0];
    onView(next);
    document.getElementById(tabId(next))?.focus();
  }

  return (
    <header className="header">
      <span className="app-name">Kite Pack</span>
      <div className="tabs" role="tablist" aria-label="Sections" onKeyDown={onKey}>
        {TABS.map(([v, name]) => {
          const on = view === v;
          return (
            <button
              key={v}
              id={tabId(v)}
              role="tab"
              aria-selected={on}
              tabIndex={on || (view === "settings" && v === "pack") ? 0 : -1}
              className="tab"
              onClick={() => onView(v)}
            >
              {name}
              {v === "pack" && badge && <span className={`tab-badge ${badge.tone}`}>{badge.text}</span>}
            </button>
          );
        })}
      </div>
      <div className="header-actions">
        <button className="ps5-chip" onClick={() => onView("ps5")}>
          <span className={`dot ${dot}`} aria-hidden="true" />
          <span className="chip-name">PS5</span>
          <span className="chip-note">{words}</span>
        </button>
        <button
          className={view === "settings" ? "icon-button on" : "icon-button"}
          aria-label="Settings"
          aria-pressed={view === "settings"}
          onClick={() => onView("settings")}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
            <path d="M4 7h10M18 7h2M4 17h4M12 17h8" />
            <circle cx="16" cy="7" r="2" />
            <circle cx="10" cy="17" r="2" />
          </svg>
        </button>
      </div>
    </header>
  );
}
