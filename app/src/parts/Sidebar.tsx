import type { MouseEvent, ReactNode } from "react";
import type { Badge } from "../model";
import type { Status, View } from "../types";

interface Props {
  view: View;
  status: Status | null;
  /** How the running job stands, shown on the page it runs on. */
  badge: Badge | null;
  badgeOn: View | null;
  onView: (view: View) => void;
}

/** The status dot's tone and words for each PS5 state. */
export function chip(status: Status | null): [string, string] {
  switch (status?.state) {
    case "running":
      return ["ok", "Helper running"];
    case "notReachable":
      return ["warn", "Not reachable"];
    case "refused":
      return ["warn", "Can't connect"];
    case "notSetUp":
      return ["off", "Not set up"];
    default:
      return ["off", "Checking"];
  }
}

const ICONS: Record<string, ReactNode> = {
  install: (
    <>
      <path d="M12 3v12M7 10l5 5 5-5" />
      <path d="M4 17v3h16v-3" />
    </>
  ),
  pack: (
    <>
      <path d="M21 8l-9-5-9 5v8l9 5 9-5z" />
      <path d="M3 8l9 5 9-5M12 13v8" />
    </>
  ),
  history: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </>
  ),
  games: (
    <>
      <rect x="3" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="3" width="7" height="7" rx="1.5" />
      <rect x="3" y="14" width="7" height="7" rx="1.5" />
      <rect x="14" y="14" width="7" height="7" rx="1.5" />
    </>
  ),
  settings: (
    <>
      <path d="M4 7h10M18 7h2M4 17h4M12 17h8" />
      <circle cx="16" cy="7" r="2" />
      <circle cx="10" cy="17" r="2" />
    </>
  ),
};

function Icon({ name }: { name: string }) {
  return (
    <svg className="side-icon" aria-hidden="true" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      {ICONS[name]}
    </svg>
  );
}

/** The app's sections: what you do, then what is on the PS5, then Settings. */
export function Sidebar({ view, status, badge, badgeOn, onView }: Props) {
  const [tone, words] = chip(status);
  function item(v: View, label: string, note?: string) {
    const go = (e: MouseEvent) => {
      e.preventDefault();
      onView(v);
    };
    const on = view === v;
    return (
      <a href={`#${v}`} className="side-item" aria-current={on ? "page" : undefined} onClick={go}>
        <Icon name={v} />
        <span className="side-words">
          <span className="side-label">{label}</span>
          {note && <span className="side-note">{note}</span>}
        </span>
        {badge && badgeOn === v && <span className={`side-badge ${badge.tone}`}>{badge.text}</span>}
      </a>
    );
  }
  return (
    <aside className="side">
      <div className="side-brand">
        <span className="side-logo" aria-hidden="true">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round">
            <path d="M12 2l7 8-7 12-7-12z" />
            <path d="M5 10h14M12 2v20" />
          </svg>
        </span>
        <span className="side-name">Kite Pack</span>
      </div>
      <nav className="side-nav" aria-label="Sections">
        {item("install", "Install", "Stream a .pkg to the PS5")}
        {item("pack", "Pack", "Compress and send")}
        {item("history", "History")}
      </nav>
      <button className="side-group" onClick={() => onView("settings")}>
        <span>PS5</span>
        <span className={`dot ${tone}`} aria-hidden="true" />
        <span className="side-status">{words}</span>
      </button>
      <nav className="side-nav" aria-label="PS5">
        {item("games", "My games")}
      </nav>
      <div className="side-fill" />
      <nav className="side-nav" aria-label="App">
        {item("settings", "Settings")}
      </nav>
    </aside>
  );
}
