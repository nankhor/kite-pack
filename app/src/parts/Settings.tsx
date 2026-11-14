import type { Found, Settings, StartOutcome, Status } from "../types";
import { Field, Ps5Connection } from "./Ps5";

interface Props {
  settings: Settings;
  status: Status | null;
  /** Kite Pack's own version; null until read. */
  version: string | null;
  /** The dlc_emu release the app carries; null until read. */
  dlcEmuVersion: string | null;
  onSave: (patch: Partial<Settings>) => Promise<void>;
  onPickFolder: () => Promise<string | null>;
  onDiscover: () => Promise<Found[]>;
  onStartHelper: () => Promise<StartOutcome>;
}

/** Behind the gear: the PS5 connection, where things go, and which versions are running. */
export function SettingsPage({ settings, status, version, dlcEmuVersion, onSave, onPickFolder, onDiscover, onStartHelper }: Props) {
  async function pickFolder() {
    const dir = await onPickFolder();
    if (dir) await onSave({ packedFolder: dir });
  }

  return (
    <section className="settings" aria-label="Settings">
      <div className="settings-head">
        <h1>Settings</h1>
      </div>
      <div className="settings-grid">
        <Ps5Connection
          settings={settings}
          status={status}
          onSave={onSave}
          onDiscover={onDiscover}
          onStartHelper={onStartHelper}
        />

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
          <div className="field">
            <span className="field-label">DLC emulator</span>
            <p className="field-hint">
              {dlcEmuVersion ? `dlc_emu ${dlcEmuVersion} is included` : "dlc_emu is included"} — added to games whose dump
              has no emulator of its own, so the DLC packed into them can be seen. Nothing to install.
            </p>
          </div>
        </div>

        <div className="column">
          <h2>About</h2>
          <dl className="facts">
            <div>
              <dt>Kite Pack</dt>
              <dd>{version ?? "…"}</dd>
            </div>
            <div>
              <dt>Kite helper on the PS5</dt>
              <dd>{status?.state === "running" ? status.version : "Not running"}</dd>
            </div>
            <div>
              <dt>DLC emulator</dt>
              <dd>{dlcEmuVersion ? `dlc_emu ${dlcEmuVersion}, included` : "Included"}</dd>
            </div>
          </dl>
          <p className="field-hint">The source and the release notes are at github.com/nankhor/kite-pack.</p>
        </div>
      </div>
    </section>
  );
}
