import { useEffect, useState } from "react";
import { formatSize, formatWhen } from "../model";
import type { HistoryEntry } from "../types";

interface Props {
  load: () => Promise<HistoryEntry[]>;
  onClear: () => Promise<void>;
  onShow: (path: string) => void;
  /** For "Today" and "Yesterday". */
  now: () => number;
}

type Filter = "all" | "install" | "pack";

/** A game's icon, or a tile with its first letter (drawn by CSS, so it isn't read as text). */
function Tile({ entry }: { entry: HistoryEntry }) {
  if (entry.iconDataUrl) return <img className="history-icon" src={entry.iconDataUrl} alt="" />;
  return <span className="history-icon placeholder" aria-hidden="true" data-letter={entry.title.slice(0, 1).toUpperCase()} />;
}

/** An install keeps no packed copy, so it has no packed size. */
const kindOf = (e: HistoryEntry): "install" | "pack" => (e.packedBytes === null ? "install" : "pack");

function what(e: HistoryEntry): string {
  if (kindOf(e) === "install") return "Installed on the PS5";
  return e.sent ? "Packed and sent" : "Packed";
}

const saved = (e: HistoryEntry) => (e.packedBytes === null ? 0 : Math.max(0, e.sourceBytes - e.packedBytes));

const FILTERS: [Filter, string][] = [
  ["all", "All"],
  ["install", "Install"],
  ["pack", "Pack"],
];

/** Everything Kite Pack finished, newest first. */
export function HistoryPage({ load, onClear, onShow, now }: Props) {
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [filter, setFilter] = useState<Filter>("all");

  useEffect(() => {
    let gone = false;
    load()
      .then((e) => !gone && setEntries(e))
      .catch(() => !gone && setEntries([]));
    return () => {
      gone = true;
    };
  }, [load]);

  async function clear() {
    await onClear();
    setEntries([]);
  }

  const all = entries ?? [];
  const shown = filter === "all" ? all : all.filter((e) => kindOf(e) === filter);
  const total = all.reduce((sum, e) => sum + saved(e), 0);
  const at = now();

  return (
    <section className="list-page history" aria-label="History">
      <div className="list-head">
        <div>
          <h1 className="page-title">History</h1>
          <p className="page-sub">
            {all.length > 0
              ? `${all.length} ${all.length === 1 ? "game" : "games"}, newest first`
              : "Everything Kite Pack finishes, newest first"}
          </p>
        </div>
        {all.length > 0 && (
          <div className="list-tools">
            {total > 0 && <span className="saved-total">{formatSize(total)} saved</span>}
            <div className="pills" role="group" aria-label="Show">
              {FILTERS.map(([f, name]) => (
                <button key={f} className={f === filter ? "pill on" : "pill"} aria-pressed={f === filter} onClick={() => setFilter(f)}>
                  {name}
                </button>
              ))}
            </div>
            <button className="small-button" onClick={() => void clear()}>
              Clear history
            </button>
          </div>
        )}
      </div>
      {entries && all.length === 0 && (
        <p className="list-empty">Nothing here yet. Each game you install or pack shows up here once it's done.</p>
      )}
      {all.length > 0 && (
        <div className="table-box">
          <table className="history-table">
            <thead>
              <tr>
                <th scope="col">Game</th>
                <th scope="col">What</th>
                <th scope="col" className="num">
                  Size
                </th>
                <th scope="col" className="num">
                  Packed
                </th>
                <th scope="col">When</th>
                <th scope="col">Where it is</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((e) => (
                <tr key={e.id}>
                  <td>
                    <span className="history-game">
                      <Tile entry={e} />
                      <span className="history-name">
                        <span className="history-title">{e.title}</span>
                        {e.dlc > 0 && <span className="history-dlc">With {e.dlc} DLC, installed on the PS5</span>}
                      </span>
                    </span>
                  </td>
                  <td>{what(e)}</td>
                  <td className="num">{formatSize(e.sourceBytes)}</td>
                  <td className="num">{e.packedBytes === null ? "–" : formatSize(e.packedBytes)}</td>
                  <td className="when">{formatWhen(e.finishedAt, at)}</td>
                  <td>
                    <span className="history-where">
                      {e.sent && (
                        <span className="on-ps5">
                          <span className="dot ok" aria-hidden="true" />
                          On the PS5
                        </span>
                      )}
                      {e.packed && (
                        <>
                          {!e.sent && <span>Packed copy kept</span>}
                          <button className="small-button" onClick={() => onShow(e.packed!)}>
                            Show folder
                          </button>
                        </>
                      )}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
