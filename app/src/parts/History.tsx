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

/** A game's icon, or a tile with its first letter. */
function Tile({ entry }: { entry: HistoryEntry }) {
  if (entry.iconDataUrl) return <img className="history-icon" src={entry.iconDataUrl} alt="" />;
  return (
    <span className="history-icon placeholder" aria-hidden="true">
      {entry.title.slice(0, 1).toUpperCase()}
    </span>
  );
}

const saved = (e: HistoryEntry) => (e.packedBytes === null ? 0 : Math.max(0, e.sourceBytes - e.packedBytes));

/** Every game packed with Kite Pack, newest first. */
export function HistoryPage({ load, onClear, onShow, now }: Props) {
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);

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

  const total = (entries ?? []).reduce((sum, e) => sum + saved(e), 0);
  const at = now();

  return (
    <section className="settings history" aria-label="History">
      <div className="settings-head">
        <h1>History</h1>
        {entries && entries.length > 0 && (
          <div className="history-sum">
            <span>
              {entries.length} {entries.length === 1 ? "game" : "games"} · <span className="saved-total">{formatSize(total)} saved</span>
            </span>
            <button className="small-button" onClick={() => void clear()}>
              Clear history
            </button>
          </div>
        )}
      </div>
      {entries && entries.length === 0 && (
        <p className="settings-note">Nothing here yet. Each game you pack shows up here once it's done.</p>
      )}
      {entries && entries.length > 0 && (
        <div className="table-box">
          <table className="history-table">
            <thead>
              <tr>
                <th scope="col">Game</th>
                <th scope="col">Packed on</th>
                <th scope="col">Size</th>
                <th scope="col">Packed</th>
                <th scope="col">Saved</th>
                <th scope="col">Where it is</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((e) => (
                <tr key={e.id}>
                  <td>
                    <span className="history-game">
                      <Tile entry={e} />
                      <span className="history-name">
                        <span className="history-title">{e.title}</span>
                        {e.dlc > 0 && (
                          <span className="history-dlc">
                            With {e.dlc} DLC, installed on the PS5
                          </span>
                        )}
                      </span>
                    </span>
                  </td>
                  <td className="when">{formatWhen(e.finishedAt, at)}</td>
                  <td className="num">{formatSize(e.sourceBytes)}</td>
                  <td className="num">{e.packedBytes === null ? "–" : formatSize(e.packedBytes)}</td>
                  <td className="num saved-cell">{e.packedBytes === null ? "–" : formatSize(saved(e))}</td>
                  <td>
                    <span className="history-where">
                      {e.sent && (
                        <span className="on-ps5">
                          <span className="dot mint" aria-hidden="true" />
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
