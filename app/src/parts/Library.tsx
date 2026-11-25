import { useCallback, useEffect, useMemo, useState } from "react";
import type { Backend } from "../backend";
import { dumpNote, formatSize } from "../model";
import type { FreeSpace, GameIcon, InstalledGame, Library as LibraryData, Measured } from "../types";

interface Props {
  backend: Backend;
  /** Shown when the library stands on its own (not inside the PS5 tab). */
  onBack?: () => void;
  /** The room left where games go is read from here (`/data/homebrew` by default). */
  destination: string;
  /** Why the console could not be read, when the chip already knows. */
  statusNote?: string | null;
  /** Sizes already read for the titles in front, by title id. */
  sizes: Record<string, Measured>;
  /** Why a size couldn't be read, by title id. */
  notes: Record<string, string>;
  /** Asks for a title's size; the app reads them a few at a time and keeps them. */
  onMeasure: (titleId: string) => void;
  /** Opens the detailed page for a title. */
  onOpen: (game: InstalledGame, icon?: string) => void;
}

type Loaded =
  | { phase: "loading" }
  | { phase: "ready"; library: LibraryData }
  | { phase: "failed"; message: string };

/** The tile of an installed title, or a stand-in with its first letter (drawn by CSS). */
function Tile({ game, icon }: { game: InstalledGame; icon?: string }) {
  if (icon) return <img className="game-tile-icon" src={icon} alt="" />;
  return <span className="game-tile-icon placeholder" aria-hidden="true" data-letter={game.title.slice(0, 1).toUpperCase()} />;
}

/** The extended drives a PS5 can have; each shows only when the console answers for it. */
const EXTENDED = ["/mnt/ext0", "/mnt/ext1"];

interface Drive {
  path: string;
  name: string;
  room: FreeSpace;
  /** Where games go: its words say so. */
  games: boolean;
}

/** One drive: how full it is, and what's left. */
function DriveCard({ drive }: { drive: Drive }) {
  const used = drive.room.total > 0 ? Math.round(((drive.room.total - drive.room.free) / drive.room.total) * 100) : 0;
  return (
    <div className="drive">
      <div className="drive-head">
        <span className="drive-name">{drive.name}</span>
        <span className="drive-path">{drive.path}</span>
      </div>
      <div className="bar" role="progressbar" aria-label={`${drive.name} used`} aria-valuenow={used} aria-valuemin={0} aria-valuemax={100}>
        <i style={{ width: `${used}%` }} />
      </div>
      <span className="drive-free">
        {formatSize(drive.room.usable)} free{drive.games ? " where games go" : ""}, of {formatSize(drive.room.total)}
      </span>
    </div>
  );
}

/** `contentVersion` "01.050.000" as "1.05"; the raw value when it doesn't read like one. */
function version(v: string | null): string {
  const m = v?.match(/^(\d+)\.(\d+)/);
  if (!m) return v ?? "–";
  return `${Number(m[1])}.${m[2].slice(0, 2).padEnd(2, "0")}`;
}

/** What is on the PS5, read over the helper's read-only calls: nothing is changed there. */
export function Library({ backend, onBack, destination, statusNote, sizes, notes, onMeasure, onOpen }: Props) {
  const [loaded, setLoaded] = useState<Loaded>({ phase: "loading" });
  const [icons, setIcons] = useState<Record<string, string>>({});
  const [query, setQuery] = useState("");
  const [drives, setDrives] = useState<Drive[]>([]);

  const load = useCallback(async () => {
    setLoaded({ phase: "loading" });
    try {
      const library = await backend.library();
      setLoaded({ phase: "ready", library });
      // Tiles are a second trip: a title with none is simply left out.
      void backend
        .libraryIcons(library.games.map((g) => g.titleId))
        .then((got: GameIcon[]) => setIcons(Object.fromEntries(got.map((i) => [i.titleId, i.dataUrl]))))
        .catch(() => {});
      // Where games go, then each extended drive that answers; one that doesn't is left out.
      void Promise.all(
        [destination, ...EXTENDED].map((path, i) =>
          backend
            .freeSpace(path)
            .then((room): Drive | null =>
              room.total > 0 ? { path, room, games: i === 0, name: i === 0 ? "Where games go" : "Extended drive" } : null,
            )
            .catch(() => null),
        ),
      ).then((found) => setDrives(found.filter((d): d is Drive => d !== null)));
    } catch (e) {
      setLoaded({ phase: "failed", message: String(e) });
    }
  }, [backend, destination]);

  useEffect(() => {
    void load();
  }, [load]);

  const games = loaded.phase === "ready" ? loaded.library.games : [];
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return games;
    return games.filter((g) => g.title.toLowerCase().includes(q) || g.titleId.toLowerCase().includes(q));
  }, [games, query]);

  // Sizes are read as soon as the list is: no button to press. The app reads a few at a time.
  useEffect(() => {
    for (const game of games) {
      if (!sizes[game.titleId] && !notes[game.titleId]) onMeasure(game.titleId);
    }
  }, [games, sizes, notes, onMeasure]);

  const heading =
    loaded.phase === "loading"
      ? "Reading the console…"
      : loaded.phase === "failed"
        ? "Couldn't read the console"
        : `${games.length} ${games.length === 1 ? "title" : "titles"} installed`;

  return (
    <section className="list-page library" aria-label="Games on the PS5">
      <div className="list-head">
        <div>
          <h1 className="page-title">My games</h1>
          <p className="page-sub">{heading}</p>
        </div>
        <div className="list-tools">
          {games.length > 0 && (
            <input
              className="search"
              type="search"
              aria-label="Search games"
              placeholder="Search by name or title id"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          )}
          <button className="small-button" onClick={() => void load()} disabled={loaded.phase === "loading"}>
            Refresh
          </button>
          {onBack && (
            <button className="small-button" onClick={onBack}>
              Back
            </button>
          )}
        </div>
      </div>

      {drives.length > 0 && (
        <section className="drives" aria-label="Space on the PS5">
          {drives.map((d) => (
            <DriveCard key={d.path} drive={d} />
          ))}
        </section>
      )}

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

      {shown.length > 0 && (
        <div className="game-grid">
          {shown.map((game) => {
            const measured = sizes[game.titleId];
            return (
              <button key={game.titleId} className="game-tile" onClick={() => onOpen(game, icons[game.titleId])}>
                <span className="game-tile-top">
                  <Tile game={game} icon={icons[game.titleId]} />
                  <span className="game-tile-name">
                    <span className="game-tile-title">{game.title}</span>
                    <span className="game-tile-id">{game.titleId}</span>
                  </span>
                </span>
                <span className="game-tile-tags">
                  <span className="chip">Version {version(game.version)}</span>
                  {game.updateInstalled && <span className="chip update">Update</span>}
                  {game.dlc.length > 0 && <span className="chip">{game.dlc.length} DLC</span>}
                  <span className="chip">{game.external ? "Extended" : "Internal"}</span>
                </span>
                <span className="game-tile-size">
                  {measured ? (
                    <>
                      <span className="game-tile-bytes">
                        {formatSize(measured.bytes)}
                        {measured.partial ? " (at least)" : ""}
                      </span>
                      <span className="size-note" title={measured.dumps.join("\n") || undefined}>
                        {dumpNote(measured)}
                      </span>
                    </>
                  ) : notes[game.titleId] ? (
                    <span role="alert" className="size-note">
                      {notes[game.titleId]}
                    </span>
                  ) : (
                    <span className="size-note">Measuring…</span>
                  )}
                </span>
              </button>
            );
          })}
        </div>
      )}
      {loaded.phase === "ready" && games.length > 0 && shown.length === 0 && (
        <p className="library-note">No title matches “{query}”.</p>
      )}
    </section>
  );
}
