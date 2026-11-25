import { useEffect, useState } from "react";
import { heroImage, makers, tags, type Candidate, type GameDetails } from "../details";
import { glowShadow, useIconGlow } from "../glow";
import { formatSize, updateLabel } from "../model";
import { baseName } from "../paths";
import type { GameInfo, InstalledGame } from "../types";
import { GamePicker } from "./GamePicker";
import { Shots } from "./Shots";

/** Where the game's details stand. */
export type DetailsView =
  | { state: "off" }
  | { state: "loading" }
  | { state: "found"; details: GameDetails }
  | { state: "none" }
  | { state: "offline" };

interface Props {
  game: GameInfo;
  source: string;
  done: boolean;
  /** Above the title once done: "Packed", or "On the PS5" when it was sent. */
  headline: string;
  /** This game as the console already has it, when it does. */
  onPs5?: InstalledGame | null;
  view: DetailsView;
  onSearch: (q: string) => Promise<Candidate[]>;
  /** An IGDB game for this one, or null for the best match again. */
  onPick: (id: number | null) => void;
  onRetry: () => void;
}

const SHOWN_DLC = 2;

/** The game: its art with the title over it, its tags, story and screenshots. */
export function Showcase({ game, source, done, headline, onPs5, view, onSearch, onPick, onRetry }: Props) {
  const glow = useIconGlow(game.iconDataUrl);
  const [allDlc, setAllDlc] = useState(false);
  const [picking, setPicking] = useState(false);
  const details = view.state === "found" ? view.details : null;
  const hero = details ? heroImage(details) : null;
  // A picture that doesn't load (offline with kept details) gives way to the icon.
  const [broken, setBroken] = useState<string | null>(null);
  useEffect(() => setBroken(null), [hero]);
  const art = hero && broken !== hero ? hero : null;

  const title = game.title ?? game.titleId ?? baseName(source);
  const update = updateLabel(game.version);
  const hasUpdate = !update.startsWith("Base game");
  const dlcs = allDlc ? game.dlcs : game.dlcs.slice(0, SHOWN_DLC);
  const meta = details ? makers(details) : "";
  const canPick = (view.state === "found" || view.state === "none") && !!game.title;

  function pick(id: number | null) {
    setPicking(false);
    onPick(id);
  }

  return (
    <section className="showcase" aria-label="Game">
      <div className="art">
        {art ? (
          <img className="art-image" src={art} alt="" onError={() => setBroken(art)} />
        ) : game.iconDataUrl ? (
          <img className="art-image blurred" src={game.iconDataUrl} alt="" />
        ) : (
          <div className="art-image plain" />
        )}
        <div className="art-scrim" aria-hidden="true" />
      </div>
      <div className="art-content">
          <div className="icon-wrap">
            {game.iconDataUrl ? (
              <img className="icon" src={game.iconDataUrl} alt="Game icon" style={{ boxShadow: glowShadow(glow, done) }} />
            ) : (
              <div className="icon icon-empty" role="img" aria-label="No icon" />
            )}
            {done && (
              <span className="done-badge" aria-hidden="true">
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#0a0d10" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M5 12.5l4.5 4.5L19 7.5" />
                </svg>
              </span>
            )}
          </div>
          <div className="art-text">
            {done && <p className="done-headline">{headline}</p>}
            <h1 className="title">{title}</h1>
            {meta && <p className="art-meta">{meta}</p>}
          </div>
        </div>

      <div className="tags-row">
        <div className="includes">
          {details && tags(details).map((t) => (
            <span className="chip" key={t}>
              {t}
            </span>
          ))}
          <span className={hasUpdate ? "chip update" : "chip"}>{update}</span>
          {dlcs.map((d) => {
            const installed = onPs5?.dlc.includes(d.label) ?? false;
            return (
              <span className={installed ? "chip installed" : "chip"} key={d.contentId}>
                <span className="chip-kind">DLC</span>
                {d.name ?? d.label}, {formatSize(d.bytes)}
                {installed && <span className="chip-note-inline">installed</span>}
              </span>
            );
          })}
          {game.dlcs.length > SHOWN_DLC && (
            <button className="chip more" onClick={() => setAllDlc(!allDlc)}>
              {allDlc ? "Fewer" : `${game.dlcs.length - SHOWN_DLC} more DLC`}
            </button>
          )}
        </div>
        {canPick && view.state === "found" && (
          <button className="link-button" aria-expanded={picking} onClick={() => setPicking(!picking)}>
            Wrong game?
          </button>
        )}
        {picking && (
          <GamePicker
            title={game.title ?? title}
            current={details?.id ?? null}
            onSearch={onSearch}
            onPick={pick}
            onClose={() => setPicking(false)}
          />
        )}
      </div>

      {details?.summary && <p className="story">{details.summary}</p>}
      {details && details.screenshots.length > 0 && <Shots shots={details.screenshots} title={details.name} />}
      {details && <p className="credit">Story, details and pictures from IGDB.</p>}

      {view.state === "none" && (
        <div className="details-note">
          <span>No game details found under this name.</span>
          {canPick && (
            <button className="small-button" aria-expanded={picking} onClick={() => setPicking(!picking)}>
              Find it
            </button>
          )}
        </div>
      )}
      {view.state === "offline" && (
        <div className="details-note" role="status">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M3 3l18 18" />
            <path d="M8.5 16.5a5 5 0 0 1 7 0" />
            <path d="M5 12.9a10 10 0 0 1 5.2-2.8" />
            <path d="M19 12.9a10 10 0 0 0-2.6-1.9" />
            <path d="M2 8.8a15 15 0 0 1 4.2-2.6" />
            <path d="M22 8.8A15 15 0 0 0 11 5" />
          </svg>
          <span>
            <strong>No story or screenshots right now.</strong> Kite Pack can't reach the game details. Packing works as usual.
          </span>
          <button className="small-button" onClick={onRetry}>
            Try again
          </button>
        </div>
      )}
    </section>
  );
}
