import { useEffect, useState } from "react";
import type { Found, Settings, StartOutcome, Status } from "../types";

interface Props {
  settings: Settings;
  status: Status | null;
  onSave: (patch: Partial<Settings>) => Promise<void>;
  onDiscover: () => Promise<Found[]>;
  onStartHelper: () => Promise<StartOutcome>;
  onPickFolder: () => Promise<string | null>;
  onDone: () => void;
}

/** A text field that saves when you leave it (or press Enter). */
function Field({ id, label, value, placeholder, hint, onSave }: { id: string; label: string; value: string; placeholder?: string; hint?: string; onSave: (v: string) => void }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      <input
        id={id}
        value={text}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
        onBlur={() => text.trim() !== value && onSave(text.trim())}
      />
      {hint && <span className="field-hint">{hint}</span>}
    </div>
  );
}

function statusLine(status: Status | null, address: string | null): [string, string] {
  switch (status?.state) {
    case "running":
      return ["mint", `Kite helper ${status.version} is running at ${address}.`];
    case "notReachable":
      return ["amber", `Nothing answers at ${address}. Turn the PS5 on and start the Kite helper.`];
    case "refused":
      return ["amber", status.reason];
    case "notSetUp":
      return ["grey", "No PS5 chosen yet. Find it on your network, or type its address."];
    default:
      return ["grey", "Checking…"];
  }
}

const STARTED: Record<StartOutcome, string> = {
  sent: "Sent to the PS5. It starts in a few seconds.",
  alreadyRunning: "It's already running.",
};

export function SettingsPage({ settings, status, onSave, onDiscover, onStartHelper, onPickFolder, onDone }: Props) {
  const [finding, setFinding] = useState(false);
  const [found, setFound] = useState<Found[] | null>(null);
  const [started, setStarted] = useState<string | null>(null);
  const [dot, line] = statusLine(status, settings.address);

  async function find() {
    setFinding(true);
    setFound(null);
    try {
      setFound(await onDiscover());
    } catch {
      setFound([]);
    } finally {
      setFinding(false);
    }
  }

  async function start() {
    setStarted(null);
    try {
      setStarted(STARTED[await onStartHelper()]);
    } catch (e) {
      setStarted(String(e));
    }
  }

  async function pickFolder() {
    const dir = await onPickFolder();
    if (dir) await onSave({ packedFolder: dir });
  }

  return (
    <section className="settings" aria-label="Settings">
      <div className="settings-head">
        <h1>Settings</h1>
        <button className="button primary" onClick={onDone}>
          Done
        </button>
      </div>

      <div className="settings-grid">
        <div className="column">
          <h2>PS5</h2>
          <p className="status-line">
            <span className={`dot ${dot}`} aria-hidden="true" />
            {line}
          </p>
          <Field
            id="address"
            label="Address"
            value={settings.address ?? ""}
            placeholder="192.168.1.42"
            onSave={(v) => void onSave({ address: v || null })}
          />
          <div className="buttons">
            <button className="button secondary" onClick={() => void find()} disabled={finding}>
              Find my PS5
            </button>
            <button className="button secondary" onClick={() => void start()} disabled={!settings.address}>
              Start the Kite helper
            </button>
          </div>
          <p className="field-hint">
            Start the Kite helper sends it to your PS5's ELF loader on port {settings.loaderPort}. Once it runs, Kite
            Pack finds and connects to it by itself.
          </p>
          {started && (
            <p role="status" className="settings-note">
              {started}
            </p>
          )}
          {finding && <p className="settings-note">Looking for your PS5…</p>}
          {found && found.length === 0 && (
            <p className="settings-note">No Kite helper found on your network. Start it on the PS5, or type its address.</p>
          )}
          {found && found.length > 0 && (
            <ul className="found">
              {found.map((f) => (
                <li key={`${f.address}:${f.port}`}>
                  <span>
                    {f.address}, Kite helper {f.version}
                  </span>
                  {f.address === settings.address ? (
                    <span className="in-use">In use</span>
                  ) : (
                    <button className="small-button" onClick={() => void onSave({ address: f.address, helperPort: f.port })}>
                      Use this PS5
                    </button>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>

        <div className="column">
          <h2>Where things go</h2>
          <Field
            id="destination"
            label="Games go to"
            value={settings.destination}
            hint="A folder on the PS5 that ShadowMount+ watches."
            onSave={(v) => void onSave({ destination: v || "/data/homebrew" })}
          />
          <div className="field">
            <span className="field-label">Packed copies go to</span>
            <div className="option-row">
              <span className="path" title={settings.packedFolder ?? undefined}>
                {settings.packedFolder ?? "Next to each game"}
              </span>
              <button className="small-button" onClick={() => void pickFolder()}>
                Change
              </button>
              {settings.packedFolder && (
                <button className="small-button" onClick={() => void onSave({ packedFolder: null })}>
                  Next to each game
                </button>
              )}
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
