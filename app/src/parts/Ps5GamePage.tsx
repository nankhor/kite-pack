import { useEffect, useState } from "react";
import type { Backend } from "../backend";
import { heroImage, makers, tags } from "../details";
import { glowShadow, useIconGlow } from "../glow";
import { dumpNote, formatSize, updateLabel } from "../model";
import type { InstalledGame, Measured } from "../types";
import type { DetailsView } from "./Showcase";
import { Shots } from "./Shots";

interface Props {
  backend: Backend;
  game: InstalledGame;
  /** Its tile, as a data: URL, when the console gave one. */
  icon: string | null;
  /** What the console says the title takes; null until read. */
  measured: Measured | null;
  /** Why its size couldn't be read, when it couldn't. */
  note: string | null;
  /** Story and pictures for the seed title, from IGDB. */
  details: DetailsView;
  onMeasure: (titleId: string) => void;
  onBack: () => void;
}

/** One installed game's page: what the console has, its size, and its story and screenshots. */
export function Ps5GamePage({ game, icon, measured, note, details, onMeasure, onBack }: Props) {
  const glow = useIconGlow(icon);
  const found = details.state === "found" ? details.details : null;
  const hero = found ? heroImage(found) : null;
  const [broken, setBroken] = useState<string | null>(null);
  useEffect(() => setBroken(null), [hero]);
  const art = hero && broken !== hero ? hero : null;

  // The size is read as soon as the page opens; there is no button to press.
  useEffect(() => {
    if (!measured && !note) onMeasure(game.titleId);
    // A later retry after a failed read is the app's business; this asks once.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [game.titleId, measured, note]);

  const update = updateLabel(game.version);
  const hasUpdate = !update.startsWith("Base game");
  const total = measured ? `${formatSize(measured.bytes)}${measured.partial ? " (at least)" : ""}` : null;

  return (
    <div className="pack-page ps5-game-page">
      <div className="pack-main">
        <div className="ps5-game-top">
          <button className="button secondary" onClick={onBack}>
            ← Back to the library
          </button>
          <span className="ps5-game-badge">On the PS5</span>
        </div>

        <section className="showcase" aria-label="Game">
          <div className="art">
            {art ? (
              <img className="art-image" src={art} alt="" onError={() => setBroken(art)} />
            ) : icon ? (
              <img className="art-image blurred" src={icon} alt="" />
            ) : (
              <div className="art-image plain" />
            )}
            <div className="art-scrim" aria-hidden="true" />
            <div className="art-content">
              <div className="icon-wrap">
                {icon ? (
                  <img className="icon" src={icon} alt="Game icon" style={{ boxShadow: glowShadow(glow, true) }} />
                ) : (
                  <div className="icon icon-empty" role="img" aria-label="No icon" />
                )}
              </div>
              <div className="art-text">
                <p className="done-headline">On the PS5</p>
                <h1 className="title">{game.title}</h1>
                {found && <p className="art-meta">{makers(found)}</p>}
              </div>
            </div>
          </div>

          <div className="tags-row">
            <div className="includes">
              {found &&
                tags(found).map((t) => (
                  <span className="chip" key={t}>
                    {t}
                  </span>
                ))}
              <span className={hasUpdate ? "chip update" : "chip"}>{update}</span>
              <span className="chip">{game.external ? "Extended storage" : "Internal storage"}</span>
              {game.dlc.length > 0 && (
                <span className="chip installed">
                  <span className="chip-kind">DLC</span>
                  {game.dlc.length} installed
                </span>
              )}
            </div>
          </div>

          {found?.summary && <p className="story">{found.summary}</p>}
          {found && found.screenshots.length > 0 && <Shots shots={found.screenshots} title={found.name} />}
          {found && <p className="credit">Story, details and pictures from IGDB.</p>}
          {details.state === "offline" && (
            <p className="credit">No story or screenshots right now: Kite Pack can't reach the game details.</p>
          )}
        </section>
      </div>

      <aside className="pack-panel" aria-label="What the PS5 has">
        <div className="panel-body">
          <dl className="facts">
            <div>
              <dt>Title ID</dt>
              <dd>{game.titleId}</dd>
            </div>
            <div>
              <dt>Version</dt>
              <dd>{game.version ?? "–"}</dd>
            </div>
            <div>
              <dt>Update</dt>
              <dd>{game.updateInstalled ? "Installed" : "None"}</dd>
            </div>
            <div>
              <dt>Storage</dt>
              <dd>{game.external ? "Extended drive" : "Internal drive"}</dd>
            </div>
            <div>
              <dt>Found in</dt>
              <dd>{game.found}</dd>
            </div>
            <div>
              <dt>Add-ons</dt>
              <dd>{game.dlc.length === 0 ? "None" : `${game.dlc.length} installed`}</dd>
            </div>
            {measured ? (
              <>
                <div>
                  <dt>Size</dt>
                  <dd>{total}</dd>
                </div>
                <div>
                  <dt>App</dt>
                  <dd>{formatSize(measured.appBytes)}</dd>
                </div>
                <div>
                  <dt>Update</dt>
                  <dd>{formatSize(measured.updateBytes)}</dd>
                </div>
                <div>
                  <dt>DLC</dt>
                  <dd>{formatSize(measured.dlcBytes)}</dd>
                </div>
                <div>
                  <dt>Dump</dt>
                  <dd>{formatSize(measured.dumpBytes)}</dd>
                </div>
                <div>
                  <dt>Where its size comes from</dt>
                  <dd>{dumpNote(measured)}</dd>
                </div>
              </>
            ) : (
              <div>
                <dt>Size</dt>
                <dd>{note ?? "Measuring…"}</dd>
              </div>
            )}
          </dl>

          {game.dlc.length > 0 && (
            <section className="ps5-dlc" aria-label="Add-ons on the PS5">
              <h2>Add-ons on the PS5</h2>
              <ul className="ps5-dlc-list">
                {game.dlc.map((label) => (
                  <li key={label}>{label}</li>
                ))}
              </ul>
            </section>
          )}
        </div>
      </aside>
    </div>
  );
}
