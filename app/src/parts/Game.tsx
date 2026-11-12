import { useState } from "react";
import { glowShadow, useIconGlow } from "../glow";
import { formatSize, updateLabel } from "../model";
import { baseName } from "../paths";
import type { GameInfo, JobView } from "../types";

interface Props {
  game: GameInfo;
  source: string;
  job: JobView | null;
  done: boolean;
  /** Above the title once done: "Packed", or "On the PS5" when it was sent. */
  headline: string;
}

const SHOWN_DLC = 2;

/** The game: its icon with a glow in its own colour, its name, facts and what the dump carries. */
export function Game({ game, source, job, done, headline }: Props) {
  const glow = useIconGlow(game.iconDataUrl);
  const [allDlc, setAllDlc] = useState(false);
  const title = game.title ?? game.titleId ?? baseName(source);
  const update = updateLabel(game.version);
  const hasUpdate = !update.startsWith("Base game");
  const dlcs = allDlc ? game.dlcs : game.dlcs.slice(0, SHOWN_DLC);
  const packed = done ? (job?.packedBytes ?? null) : null;
  const saved = packed !== null && game.totalBytes > packed ? game.totalBytes - packed : null;

  return (
    <section className="game" aria-label="Game">
      <div className="icon-wrap">
        {game.iconDataUrl ? (
          <img className="icon" src={game.iconDataUrl} alt="Game icon" style={{ boxShadow: glowShadow(glow, done) }} />
        ) : (
          <div className="icon icon-empty" role="img" aria-label="No icon" style={{ boxShadow: glowShadow(null, done) }} />
        )}
        {done && (
          <span className="done-badge" aria-hidden="true">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#0a0d10" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
              <path d="M5 12.5l4.5 4.5L19 7.5" />
            </svg>
          </span>
        )}
      </div>
      <div className="game-text">
        {done && <p className="done-headline">{headline}</p>}
        <h1 className="title">{title}</h1>
        <dl className="facts">
          {game.titleId && (
            <div>
              <dt>Title ID</dt>
              <dd>{game.titleId}</dd>
            </div>
          )}
          {game.totalBytes > 0 && (
            <div>
              <dt>Size</dt>
              <dd>{formatSize(game.totalBytes)}</dd>
            </div>
          )}
          {packed !== null && (
            <div>
              <dt>Packed</dt>
              <dd>{formatSize(packed)}</dd>
            </div>
          )}
          {saved !== null && (
            <div className="saved">
              <dt>Saved</dt>
              <dd>{formatSize(saved)}</dd>
            </div>
          )}
        </dl>
        <div className="includes">
          <span className="includes-label">Includes</span>
          <span className={hasUpdate ? "chip update" : "chip"}>{update}</span>
          {dlcs.map((d) => (
            <span className="chip" key={d.contentId}>
              <span className="chip-kind">DLC</span>
              {d.name ?? d.label}, {formatSize(d.bytes)}
            </span>
          ))}
          {game.dlcs.length > SHOWN_DLC && (
            <button className="chip more" onClick={() => setAllDlc(!allDlc)}>
              {allDlc ? "Fewer" : `${game.dlcs.length - SHOWN_DLC} more DLC`}
            </button>
          )}
        </div>
      </div>
    </section>
  );
}
