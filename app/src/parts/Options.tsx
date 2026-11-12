import type { Format, Level, PackOptions, Status } from "../types";

interface Props {
  options: PackOptions;
  /** How the PS5 stands; decides whether Send can be switched on. */
  status: Status | null;
  /** Where games go on the PS5, for the Send hint. */
  destination: string;
  onChange: (patch: Partial<PackOptions>) => void;
  onChangeFolder: () => void;
}

const FORMATS: [Format, string][] = [
  ["folder", "Folder"],
  ["exfat", "exFAT"],
  ["ffpkg", ".ffpkg"],
  ["ffpfs", ".ffpfs"],
];

const LEVELS: [Level, string, string][] = [
  ["fast", "Fast", "Quickest to pack"],
  ["medium", "Medium", "Smaller, takes longer"],
  ["max", "Max", "Smallest, takes much longer"],
];

function Pills<T extends string>({ label, items, value, onPick }: { label: string; items: [T, string][]; value: T; onPick: (v: T) => void }) {
  return (
    <div className="pills" role="group" aria-label={label}>
      {items.map(([v, name]) => (
        <button key={v} aria-pressed={v === value} className={v === value ? "pill on" : "pill"} onClick={() => onPick(v)}>
          {name}
        </button>
      ))}
    </div>
  );
}

function Switch({ id, label, hint, on, disabled, sub, onToggle }: { id: string; label: string; hint: string; on: boolean; disabled?: boolean; sub?: boolean; onToggle?: () => void }) {
  const className = ["toggle", sub && "sub", disabled && "disabled"].filter(Boolean).join(" ");
  return (
    <div className={className}>
      <button
        role="switch"
        aria-checked={on}
        aria-labelledby={`${id}-label`}
        aria-describedby={`${id}-hint`}
        disabled={disabled}
        className={on ? "switch on" : "switch"}
        onClick={onToggle}
      >
        <span className="knob" />
      </button>
      <div className="toggle-text">
        <span id={`${id}-label`} className="toggle-label">
          {label}
        </span>
        <span id={`${id}-hint`} className="toggle-hint">
          {hint}
        </span>
      </div>
    </div>
  );
}

/** Before Pack: how to pack, and what happens after. */
/** Send's hint, and whether it can be switched on, for each PS5 state. */
function sendState(status: Status | null, destination: string): [boolean, string] {
  switch (status?.state) {
    case "running":
      return [true, `Starts by itself when packing ends, into ${destination}.`];
    case "notReachable":
      return [true, "The PS5 isn't reachable right now. Sending waits for it."];
    case "refused":
      return [false, status.reason];
    default:
      return [false, "Set up your PS5 in Settings first."];
  }
}

export function Options({ options, status, destination, onChange, onChangeFolder }: Props) {
  const levelHint = LEVELS.find(([l]) => l === options.level)?.[2] ?? "";
  const [canSend, sendHint] = sendState(status, destination);
  const sending = canSend && options.send;
  return (
    <section className="options" aria-label="Options">
      <div className="column">
        <h2>Pack</h2>
        <div className="option-row">
          <span className="option-label">Save as</span>
          <Pills label="Save as" items={FORMATS} value={options.format} onPick={(format) => onChange({ format })} />
        </div>
        <div className="option-row">
          <span className="option-label">Level</span>
          <Pills label="Level" items={LEVELS.map(([v, n]) => [v, n] as [Level, string])} value={options.level} onPick={(level) => onChange({ level })} />
          <span className="option-hint">{levelHint}</span>
        </div>
        <div className="option-row">
          <span className="option-label">Save to</span>
          <span className="path" title={options.outputDir}>
            {options.outputDir}
          </span>
          <button className="small-button" onClick={onChangeFolder}>
            Change
          </button>
        </div>
      </div>
      <div className="column">
        <h2>After packing</h2>
        <Switch
          id="delete-original"
          label="Delete the original"
          hint="It goes to the Trash after the packed copy is checked."
          on={options.deleteOriginal}
          onToggle={() => onChange({ deleteOriginal: !options.deleteOriginal })}
        />
        <Switch
          id="send"
          label="Send to PS5"
          hint={sendHint}
          on={sending}
          disabled={!canSend}
          onToggle={() => onChange({ send: !options.send })}
        />
        {sending && (
          <Switch
            id="delete-packed"
            label="Delete the packed folder"
            hint="It goes to the Trash once the PS5 confirms every file."
            on={options.deletePacked}
            sub
            onToggle={() => onChange({ deletePacked: !options.deletePacked })}
          />
        )}
        {sending && options.deleteOriginal && options.deletePacked && (
          <p role="status" className="only-ps5">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden="true">
              <path d="M12 8v5M12 16.5v.5" />
              <circle cx="12" cy="12" r="9" />
            </svg>
            Only the copy on the PS5 will be left.
          </p>
        )}
      </div>
    </section>
  );
}
