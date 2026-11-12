import type { Status } from "../types";

interface Props {
  status: Status | null;
  onOpenSettings: () => void;
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

/** App name, the PS5 chip and Settings. Both open Settings. */
export function Header({ status, onOpenSettings }: Props) {
  const [dot, words] = chip(status);
  return (
    <header className="header">
      <span className="app-name">Kite Pack</span>
      <div className="header-actions">
        <button className="ps5-chip" onClick={onOpenSettings}>
          <span className={`dot ${dot}`} aria-hidden="true" />
          <span className="chip-name">PS5</span>
          <span className="chip-note">{words}</span>
        </button>
        <button className="icon-button" aria-label="Settings" onClick={onOpenSettings}>
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
