import { useEffect, useRef, useState } from "react";
import { formatSize } from "../model";
import type { BundleItem, BundlePlan, SmpStatus } from "../types";

interface Props {
  /** What the picked DLC and backport would add; null until read. */
  plan: BundlePlan | null;
  /** Reading the picks. */
  busy: boolean;
  /** How many DLC/backport are picked (so a failed plan read can say so). */
  picked: number;
  /** DLC labels the console already has installed. */
  installed: string[];
  /** ShadowMount+, when it answers. */
  smp: SmpStatus | null;
  /** The DLC packages ShadowMount+ will install natively. */
  nativeDlc: string[];
  /** Sending a backport to ShadowMount+ right now. */
  installingBackport: boolean;
  /** Sends the picked backport to ShadowMount+, which applies it at the next launch. */
  onInstallBackport: () => void;
  /** Stands in for a DLC that has no icon of its own. */
  gameIcon: string | null;
  onAddPackages: () => void;
  onAddFolder: () => void;
  onAddBackport: () => void;
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

const CARET = (
  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M6 9l6 6 6-6" />
  </svg>
);

const name = (item: BundleItem) => item.name;
const isInstalled = (item: BundleItem, installed: string[]) =>
  item.label !== null && installed.some((l) => l.toLowerCase() === item.label!.toLowerCase());

/** What an item's badge says, and why. */
function badge(item: BundleItem, installed: string[], nativeDlc: string[]): [string, string] {
  if (item.problem) return ["Can't go in", "bad"];
  if (isInstalled(item, installed)) return ["On the PS5", "have"];
  if (nativeDlc.includes(item.path)) return ["Installs on the PS5", "kind"];
  if (item.kind === "backport") return ["Backport", "kind"];
  if (item.route === "install") return ["After install", "wait"];
  return ["In the pack", "kind"];
}

function Icon({ item, gameIcon }: { item: BundleItem; gameIcon: string | null }) {
  const src = item.iconDataUrl ?? (item.kind === "dlc" ? gameIcon : null);
  if (src) return <img className="dlc-icon" src={src} alt="" />;
  return <span className="dlc-icon blank" aria-hidden="true" />;
}

/** DLC and a backport folded into the packed copy, before it is written. */
export function Dlc({
  plan,
  busy,
  picked,
  installed,
  smp,
  nativeDlc,
  installingBackport,
  gameIcon,
  onAddPackages,
  onAddFolder,
  onAddBackport,
  onInstallBackport,
  onRemove,
  onRemoveAll,
}: Props) {
  const [open, setOpen] = useState(false);
  const [menu, setMenu] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const items = plan?.items ?? [];
  const shown = open && items.length > 0;
  const dlc = items.filter((i) => i.kind === "dlc");
  const total = dlc.reduce((sum, d) => sum + d.bytes, 0);
  const toInstall = items.filter((i) => i.route === "install" && !isInstalled(i, installed));

  // A click anywhere else closes the Add DLC menu.
  useEffect(() => {
    if (!menu) return;
    const away = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenu(false);
    };
    document.addEventListener("mousedown", away);
    return () => document.removeEventListener("mousedown", away);
  }, [menu]);

  let hint = "Add DLC packages, a DLC folder, or a backport. They go inside the packed copy, so the PS5 gets them with the game.";
  if (busy) hint = "Reading what you picked…";
  else if (picked > 0 && !plan) hint = "Couldn't read the DLC you picked: remove them and add them again.";
  else if (items.length > 0) hint = "Added to the packed copy.";
  if (nativeDlc.length > 0) {
    hint = `${nativeDlc.length} of these install on the PS5 through Sony's installer once the game is sent.`;
  }
  if (plan?.problem) hint = plan.problem;
  else if (toInstall.length > 0) {
    hint =
      "An image can't carry DLC: unpack it to a folder first, or install the game as a PKG so Sony's installer can add the DLC.";
  }
  const backport = items.find((i) => i.kind === "backport");

  const label = (n: number) => `${n} ${n === 1 ? "add-on" : "add-ons"}`;

  return (
    <section className={plan?.problem ? "dlc blocked" : "dlc"} aria-label="DLC and a backport">
      <div className="dlc-head">
        <h2>Inside the packed copy</h2>
        <span className="dlc-hint">{hint}</span>
      </div>
      <div className="dlc-controls">
        {items.length > 0 && (
          <button className="dlc-summary" aria-expanded={shown} aria-controls="dlc-list" onClick={() => setOpen(!open)}>
            <span className="dlc-count">
              {label(dlc.length)}
              {items.some((i) => i.kind === "backport") ? " · 1 backport" : ""}
            </span>
            {dlc.length > 0 && <span className="dlc-total">{formatSize(total)}</span>}
            {CARET}
          </button>
        )}
        <div className="add-dlc" ref={menuRef}>
          <button
            className="add-button"
            aria-haspopup="menu"
            aria-expanded={menu}
            disabled={busy}
            onClick={() => setMenu(!menu)}
          >
            {PLUS}Add DLC{CARET}
          </button>
          {menu && (
            <div className="add-menu" role="menu" aria-label="Add DLC">
              <button
                role="menuitem"
                onClick={() => {
                  setMenu(false);
                  onAddPackages();
                }}
              >
                Packages…
                <span>One or more .pkg files</span>
              </button>
              <button
                role="menuitem"
                onClick={() => {
                  setMenu(false);
                  onAddFolder();
                }}
              >
                Folder…
                <span>A DLC dump folder, or a folder of packages</span>
              </button>
            </div>
          )}
        </div>
        <button className="add-button" onClick={onAddBackport} disabled={busy}>
          {PLUS}Add backport
        </button>
        {items.length > 0 && (
          <p className="dlc-closing">
            Closing the app while packing starts the pack over; sending picks up where it stopped.
          </p>
        )}
        {backport && smp && (
          <button
            className="add-button"
            onClick={onInstallBackport}
            disabled={installingBackport || busy}
            title="Sends it to ShadowMount+, which applies it the next time the game starts."
          >
            {installingBackport ? "Installing…" : "Install backport on the PS5"}
          </button>
        )}
        {shown && <div className="sheet-backdrop" aria-hidden="true" onMouseDown={() => setOpen(false)} />}
        {shown && (
          <div
            id="dlc-list"
            className="popover sheet"
            role="dialog"
            aria-modal="true"
            aria-label="What goes inside the packed copy"
            onKeyDown={(e) => e.key === "Escape" && setOpen(false)}
          >
            <div className="popover-head">
              <span className="popover-title">
                {label(dlc.length)}
                {dlc.length > 0 && <span className="popover-total"> · {formatSize(total)}</span>}
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
              <button className="round-button" aria-label="Close the list" autoFocus onClick={() => setOpen(false)}>
                {X}
              </button>
            </div>
            <ol className="popover-list">
              {items.map((item, i) => {
                const [word, tone] = badge(item, installed, nativeDlc);
                return (
                  <li key={`${item.kind}:${item.path}`}>
                    <span className="popover-n">{i + 1}</span>
                    <Icon item={item} gameIcon={gameIcon} />
                    <span className="popover-name">{name(item)}</span>
                    <span className={`dlc-badge ${tone}`}>{word}</span>
                    <span className="popover-size">{item.bytes > 0 ? formatSize(item.bytes) : ""}</span>
                    <button
                      className="round-button"
                      aria-label={`Remove ${name(item)}`}
                      onClick={() => onRemove(item.path)}
                    >
                      {X}
                    </button>
                  </li>
                );
              })}
            </ol>
            {plan?.emulatorFrom === "builtin" && dlc.length > 0 && (
              <p className="popover-note">The DLC emulator Kite Pack carries goes into the game's fakelib/.</p>
            )}
            {toInstall.length > 0 && (
              <p className="popover-note">
                {nativeDlc.length > 0
                  ? "These install on the PS5 after the game is sent."
                  : "Marked “After install” will not be packed: no package installer is available."}
              </p>
            )}
          </div>
        )}
      </div>
    </section>
  );
}
