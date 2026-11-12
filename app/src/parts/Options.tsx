import type { Format, Level, PackOptions } from "../types";

interface Props {
  options: PackOptions;
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

function Switch({ id, label, hint, on, disabled, onToggle }: { id: string; label: string; hint: string; on: boolean; disabled?: boolean; onToggle?: () => void }) {
  return (
    <div className={disabled ? "toggle disabled" : "toggle"}>
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
export function Options({ options, onChange, onChangeFolder }: Props) {
  const levelHint = LEVELS.find(([l]) => l === options.level)?.[2] ?? "";
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
        <Switch id="send" label="Send to PS5" hint="Needs the Kite helper on your PS5. It comes in the next version." on={false} disabled />
      </div>
    </section>
  );
}
