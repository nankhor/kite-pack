import { useEffect, useState } from "react";
import { igdbImage, type Candidate } from "../details";

interface Props {
  /** The name to search for first. */
  title: string;
  /** The IGDB game shown now, if one is. */
  current: number | null;
  onSearch: (q: string) => Promise<Candidate[]>;
  /** An IGDB game, or null for the best match again. */
  onPick: (id: number | null) => void;
  onClose: () => void;
}

/** "Wrong game?": search IGDB by name and pick the right one. */
export function GamePicker({ title, current, onSearch, onPick, onClose }: Props) {
  const [q, setQ] = useState(title);
  const [results, setResults] = useState<Candidate[] | null>(null);
  const [failed, setFailed] = useState(false);

  async function search(text: string) {
    const query = text.trim();
    if (!query) return;
    setFailed(false);
    setResults(null);
    try {
      setResults(await onSearch(query));
    } catch {
      setFailed(true);
      setResults([]);
    }
  }

  useEffect(() => {
    void search(title);
    // Only the first search runs by itself.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div
      className="popover picker"
      role="dialog"
      aria-label="Find the right game"
      onKeyDown={(e) => e.key === "Escape" && onClose()}
    >
      <form
        className="picker-search"
        onSubmit={(e) => {
          e.preventDefault();
          void search(q);
        }}
      >
        <label className="visually-hidden" htmlFor="picker-q">
          Game name
        </label>
        <input id="picker-q" value={q} autoFocus onChange={(e) => setQ(e.target.value)} spellCheck={false} />
        <button className="small-button" type="submit">
          Search
        </button>
      </form>
      {results === null ? (
        <p className="picker-note">Searching…</p>
      ) : failed ? (
        <p className="picker-note">Can't reach the game details right now.</p>
      ) : results.length === 0 ? (
        <p className="picker-note">No game by that name. Try a shorter one.</p>
      ) : (
        <ul className="picker-list" aria-label="Games">
          {results.map((c) => (
            <li key={c.id}>
              <button className="picker-item" aria-current={c.id === current ? "true" : undefined} onClick={() => onPick(c.id)}>
                {c.cover ? (
                  <img className="picker-cover" src={igdbImage(c.cover, "cover_small")} alt="" loading="lazy" />
                ) : (
                  <span className="picker-cover blank" aria-hidden="true" />
                )}
                <span className="picker-name">{c.name}</span>
                {c.year && <span className="picker-year">{c.year}</span>}
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="picker-foot">
        <button className="small-button" onClick={() => onPick(null)}>
          Use the best match
        </button>
        <button className="small-button" onClick={onClose}>
          Close
        </button>
      </div>
    </div>
  );
}
