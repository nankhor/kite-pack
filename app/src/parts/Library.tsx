import { useCallback, useEffect, useMemo, useState } from "react";
import type { Backend } from "../backend";
import { formatSize } from "../model";
import type { GameIcon, InstalledGame, Library as LibraryData, Measured } from "../types";

interface Props {
  backend: Backend;
  /** Shown when the library stands on its own (not inside the PS5 tab). */
  onBack?: () => void;
  /** The room left where games go is read from here (`/data/homebrew` by default). */
  destination: string;
  /** Why the console could not be read, when the chip already knows. */
  statusNote?: string | null;
}

type Loaded =
  | { phase: "loading" }
  | { phase: "ready"; library: LibraryData }
  | { phase: "failed"; message: string };

/** The tile of an installed title, or a stand-in with its first letter. */
function Tile({ game, icon }: { game: InstalledGame; icon?: string }) {
  if (icon) return <img className="history-icon" src={icon} alt="" />;
  return (
    <span className="history-icon placeholder" aria-hidden="true">
      {game.title.slice(0, 1).toUpperCase()}
    </span>
  );
}

/** `contentVersion` "01.050.000" as "1.05"; the raw value when it doesn't read like one. */
function version(v: string | null): string {
  const m = v?.match(/^(\d+)\.(\d+)/);
  if (!m) return v ?? "–";
  return `${Number(m[1])}.${m[2].slice(0, 2).padEnd(2, "0")}`;
}

/** What is on the PS5, read over the helper's read-only calls: nothing is changed there. */
export function Library({ backend, onBack, destination, statusNote }: Props) {
  const [loaded, setLoaded] = useState<Loaded>({ phase: "loading" });
  const [icons, setIcons] = useState<Record<string, string>>({});
  const [sizes, setSizes] = useState<Record<string, Measured>>({});
  const [measuring, setMeasuring] = useState<string | null>(null);
  const [notes, setNotes] = useState<Record<string, string>>({});
  const [query, setQuery] = useState("");
  const [free, setFree] = useState<number | null>(null);

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
      void backend
        .freeSpace(destination)
        .then((room) => setFree(room.usable))
        .catch(() => setFree(null));
    } catch (e) {
      setLoaded({ phase: "failed", message: String(e) });
    }
  }, [backend, destination]);

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
          <p className="library-sub">
            <span>{heading}</span>
            {free !== null && (
              <span>
                {" · "}
                {formatSize(free)} free where games go
              </span>
            )}
          </p>
        </div>
        <div className="buttons">
          <button className="button secondary" onClick={() => void load()} disabled={loaded.phase === "loading"}>
            Refresh
          </button>
          {onBack && (
            <button className="button secondary" onClick={onBack}>
              Back
            </button>
          )}
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

      {shown.length > 0 && (
        <div className="table-box">
          <table className="history-table library-table">
            <thead>
              <tr>
                <th scope="col">Game</th>
                <th scope="col">Version</th>
                <th scope="col">Update</th>
                <th scope="col">DLC</th>
                <th scope="col">Storage</th>
                <th scope="col">Size</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((game) => {
                const measured = sizes[game.titleId];
                return (
                  <tr key={game.titleId}>
                    <td>
                      <span className="history-game">
                        <Tile game={game} icon={icons[game.titleId]} />
                        <span className="history-name">
                          <span className="history-title">{game.title}</span>
                          <span className="history-dlc">{game.titleId}</span>
                          {notes[game.titleId] && (
                            <span role="alert" className="game-card-note">
                              {notes[game.titleId]}
                            </span>
                          )}
                        </span>
                      </span>
                    </td>
                    <td className="num">{version(game.version)}</td>
                    <td>{game.updateInstalled ? <span className="chip update">Installed</span> : <span className="dim">–</span>}</td>
                    <td className="num">{game.dlc.length > 0 ? `${game.dlc.length} DLC` : <span className="dim">–</span>}</td>
                    <td>{game.external ? "Extended" : "Internal"}</td>
                    <td className="num">
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
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {loaded.phase === "ready" && games.length > 0 && shown.length === 0 && (
        <p className="library-note">No title matches “{query}”.</p>
      )}
    </section>
  );
}
