import { useCallback, useEffect, useMemo, useState } from "react";
import type { Backend } from "../backend";
import { formatSize } from "../model";
import type { GameIcon, InstalledGame, Library as LibraryData, Measured } from "../types";

interface Props {
  backend: Backend;
  onBack: () => void;
  /** Why the console could not be read, when the chip already knows. */
  statusNote?: string | null;
}

type Loaded =
  | { phase: "loading" }
  | { phase: "ready"; library: LibraryData }
  | { phase: "failed"; message: string };

/** The tiles of an installed title, or a stand-in with its first letter. */
function Tile({ game, icon }: { game: InstalledGame; icon?: string }) {
  if (icon) return <img className="game-icon" src={icon} alt="" />;
  return (
    <span className="game-icon placeholder" aria-hidden="true">
      {game.title.slice(0, 1).toUpperCase()}
    </span>
  );
}

/** The chips under a title: its update, its add-ons, its drive. */
function Chips({ game }: { game: InstalledGame }) {
  return (
    <div className="game-chips">
      {game.updateInstalled && <span className="chip update">Update installed</span>}
      {game.dlc.length > 0 && (
        <span className="chip">
          {game.dlc.length} DLC
        </span>
      )}
      {game.external && <span className="chip kind">Extended storage</span>}
      {game.version && <span className="chip kind">v{game.version}</span>}
    </div>
  );
}

/** What is on the PS5, read over the helper's read-only calls: nothing is changed there. */
export function Library({ backend, onBack, statusNote }: Props) {
  const [loaded, setLoaded] = useState<Loaded>({ phase: "loading" });
  const [icons, setIcons] = useState<Record<string, string>>({});
  const [sizes, setSizes] = useState<Record<string, Measured>>({});
  const [measuring, setMeasuring] = useState<string | null>(null);
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [query, setQuery] = useState("");

  const load = useCallback(async () => {
    setLoaded({ phase: "loading" });
    setSizes({});
    setNotes({});
    try {
      const library = await backend.library();
      setLoaded({ phase: "ready", library });
      // Tiles are a second trip: a title with none is simply left out.
      void backend
        .libraryIcons(library.games.map((g) => g.titleId))
        .then((got: GameIcon[]) => setIcons(Object.fromEntries(got.map((i) => [i.titleId, i.dataUrl]))))
        .catch(() => {});
    } catch (e) {
      setLoaded({ phase: "failed", message: String(e) });
    }
  }, [backend]);

  useEffect(() => {
    void load();
  }, [load]);

  async function measure(game: InstalledGame) {
    setMeasuring(game.titleId);
    setNotes((n) => ({ ...n, [game.titleId]: "" }));
    try {
      const m = await backend.measureApp(game.titleId);
      setSizes((s) => ({ ...s, [game.titleId]: m }));
    } catch (e) {
      setNotes((n) => ({ ...n, [game.titleId]: String(e) }));
    } finally {
      setMeasuring(null);
    }
  }

  const games = loaded.phase === "ready" ? loaded.library.games : [];
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return games;
    return games.filter((g) => g.title.toLowerCase().includes(q) || g.titleId.toLowerCase().includes(q));
  }, [games, query]);

  const heading =
    loaded.phase === "loading"
      ? "Reading the console…"
      : loaded.phase === "failed"
        ? "Couldn't read the console"
        : `${games.length} ${games.length === 1 ? "title" : "titles"} installed`;

  return (
    <section className="library" aria-label="Games on the PS5">
      <div className="library-top">
        <div className="library-heading">
          <h1 className="library-title">On the PS5</h1>
          <p className="library-sub">{heading}</p>
        </div>
        <div className="buttons">
          <button className="button secondary" onClick={() => void load()} disabled={loaded.phase === "loading"}>
            Refresh
          </button>
          <button className="button secondary" onClick={onBack}>
            Back
          </button>
        </div>
      </div>

      {loaded.phase === "failed" && (
        <p role="alert" className="message">
          {loaded.message}
          {statusNote ? ` ${statusNote}` : ""}
        </p>
      )}
      {loaded.phase === "ready" && loaded.library.truncated && (
        <p className="library-note">Only the first 400 titles are listed.</p>
      )}
      {loaded.phase === "ready" && games.length === 0 && (
        <p className="library-note">
          Nothing installed, or the helper only saw folders it can't name. Send a game to see it here.
        </p>
      )}

      {games.length > 0 && (
        <input
          className="library-search"
          type="search"
          aria-label="Search games"
          placeholder="Search by name or title id"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      )}

      <ul className="library-grid">
        {shown.map((game) => {
          const measured = sizes[game.titleId];
          return (
            <li className="game-card" key={game.titleId}>
              <Tile game={game} icon={icons[game.titleId]} />
              <div className="game-card-text">
                <p className="game-card-title">{game.title}</p>
                <p className="game-card-id">{game.titleId}</p>
                <Chips game={game} />
                <div className="game-card-size">
                  {measured ? (
                    <>
                      {formatSize(measured.bytes)}
                      {measured.partial ? " (at least)" : ""}
                    </>
                  ) : (
                    <button
                      className="small-button"
                      onClick={() => void measure(game)}
                      disabled={measuring === game.titleId}
                    >
                      {measuring === game.titleId ? "Measuring…" : "Measure"}
                    </button>
                  )}
                </div>
                {notes[game.titleId] && (
                  <p role="alert" className="game-card-note">
                    {notes[game.titleId]}
                  </p>
                )}
              </div>
            </li>
          );
        })}
      </ul>
      {loaded.phase === "ready" && games.length > 0 && shown.length === 0 && (
        <p className="library-note">No title matches “{query}”.</p>
      )}
    </section>
  );
}
