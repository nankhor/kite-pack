import type { StepRow } from "../model";

function StepIcon({ state }: { state: StepRow["state"] }) {
  switch (state) {
    case "done":
      return (
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none">
          <circle cx="12" cy="12" r="11" fill="var(--mint)" />
          <path d="M7 12.5l3.2 3.2L17 9" stroke="#0a0d10" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      );
    case "active":
      return (
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none">
          <circle cx="12" cy="12" r="10" stroke="#ffffff" strokeWidth="2" />
          <circle cx="12" cy="12" r="5" fill="#ffffff" />
        </svg>
      );
    case "paused":
      return (
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none">
          <circle cx="12" cy="12" r="10" stroke="var(--amber)" strokeWidth="2" />
          <path d="M10 8.5v7M14 8.5v7" stroke="var(--amber)" strokeWidth="2.2" strokeLinecap="round" />
        </svg>
      );
    case "failed":
      return (
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none">
          <circle cx="12" cy="12" r="10" stroke="var(--fail)" strokeWidth="2" />
          <path d="M12 7.5v6M12 16.5v.5" stroke="var(--fail)" strokeWidth="2.2" strokeLinecap="round" />
        </svg>
      );
    default:
      return (
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none">
          <circle cx="12" cy="12" r="10" stroke="rgba(255,255,255,0.4)" strokeWidth="2" strokeDasharray="3 3" />
        </svg>
      );
  }
}

/** After Pack: each chosen step, in order. */
export function Steps({ rows }: { rows: StepRow[] }) {
  return (
    <ol className="steps" aria-label="Steps">
      {rows.map((row) => (
        <li key={row.label} className={`step ${row.state}`}>
          <div className="step-line">
            <span className="step-icon" aria-hidden="true">
              <StepIcon state={row.state} />
            </span>
            <span className="step-label">{row.label}</span>
            <span className="step-status">{row.status}</span>
          </div>
          {row.bar !== null && (
            <div
              className="bar"
              role="progressbar"
              aria-label={row.label}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={row.bar}
            >
              <div className="bar-fill" style={{ width: `${row.bar}%` }} />
            </div>
          )}
        </li>
      ))}
    </ol>
  );
}
