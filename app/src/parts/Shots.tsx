import { useState } from "react";
import { igdbImage } from "../details";

const SHOWN = 4;

const X = (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" aria-hidden="true">
    <path d="M6 6l12 12M18 6L6 18" />
  </svg>
);
const LEFT = (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M15 6l-6 6 6 6" />
  </svg>
);
const RIGHT = (
  <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M9 6l6 6-6 6" />
  </svg>
);

/** A row of screenshots; any of them opens a big view with arrows. */
export function Shots({ shots, title }: { shots: string[]; title: string }) {
  const [at, setAt] = useState<number | null>(null);
  const n = shots.length;
  const go = (i: number) => setAt(((i % n) + n) % n);

  return (
    <section className="shots" aria-label="Screenshots">
      <div className="shots-head">
        <h2>Screenshots</h2>
        <span className="shots-count">{n}</span>
      </div>
      <div className="shots-row">
        {shots.slice(0, SHOWN).map((id, i) => {
          const more = i === SHOWN - 1 && n > SHOWN;
          return (
            <button
              key={id}
              className="shot"
              aria-label={more ? `See all ${n} screenshots` : `Screenshot ${i + 1} of ${n}`}
              onClick={() => setAt(i)}
            >
              <img src={igdbImage(id, "screenshot_med")} alt="" loading="lazy" />
              {more && (
                <span className="shot-more" aria-hidden="true">
                  +{n - SHOWN + 1}
                </span>
              )}
            </button>
          );
        })}
      </div>
      {at !== null && (
        <div
          className="lightbox"
          role="dialog"
          aria-modal="true"
          aria-label={`${title}: screenshots`}
          onKeyDown={(e) => {
            if (e.key === "Escape") setAt(null);
            else if (e.key === "ArrowLeft") go(at - 1);
            else if (e.key === "ArrowRight") go(at + 1);
          }}
        >
          <button className="round-button lightbox-close" aria-label="Close" autoFocus onClick={() => setAt(null)}>
            {X}
          </button>
          <img className="lightbox-image" src={igdbImage(shots[at], "1080p")} alt={`Screenshot ${at + 1} of ${n}`} />
          {n > 1 && (
            <div className="lightbox-nav">
              <button className="round-button" aria-label="Previous screenshot" onClick={() => go(at - 1)}>
                {LEFT}
              </button>
              <span className="lightbox-count">
                {at + 1} of {n}
              </span>
              <button className="round-button" aria-label="Next screenshot" onClick={() => go(at + 1)}>
                {RIGHT}
              </button>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
