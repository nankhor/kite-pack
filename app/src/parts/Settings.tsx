import type { Found, Settings, SmpStatus, StartOutcome, Status } from "../types";
import { useState } from "react";
import type { UpdatePrefs } from "../updates";
import { Switch } from "./Options";
import { Field, Ps5Connection } from "./Ps5";

interface Props {
  settings: Settings;
  status: Status | null;
  /** Kite Pack's own version; null until read. */
  version: string | null;
  /** The dlc_emu release the app carries; null until read. */
  dlcEmuVersion: string | null;
  /** ShadowMount+, when it answers. */
  smp?: SmpStatus | null;
  /** Why ShadowMount+ couldn't be asked, when it couldn't. */
  smpError?: string | null;
  /** Whether the Pack page looks games up on IGDB. */
  detailsOn: boolean;
  onDetailsOn: (on: boolean) => void;
  updatePrefs: UpdatePrefs;
  onUpdatePrefs: (prefs: UpdatePrefs) => void;
  /** Looks for a newer version; says what it found. */
  onCheckUpdate: () => Promise<string>;
  onSave: (patch: Partial<Settings>) => Promise<void>;
  onPickFolder: () => Promise<string | null>;
  onDiscover: () => Promise<Found[]>;
  onStartHelper: () => Promise<StartOutcome>;
}

/** Behind the gear: the PS5 connection, where things go, and which versions are running. */
export function SettingsPage({ settings, status, version, dlcEmuVersion, smp, smpError, detailsOn, onDetailsOn, updatePrefs, onUpdatePrefs, onCheckUpdate, onSave, onPickFolder, onDiscover, onStartHelper }: Props) {
  const [checkNote, setCheckNote] = useState<string | null>(null);

  async function checkNow() {
    setCheckNote("Checking…");
    setCheckNote(await onCheckUpdate());
  }

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
          <h2>Game details</h2>
          <Switch
            id="details"
            label="Show game details"
            hint="The story, screenshots and art from IGDB, through Kite Pack's own server. Only the game's name is sent."
            on={detailsOn}
            onToggle={() => onDetailsOn(!detailsOn)}
          />
          <h2>Updates</h2>
          <Switch
            id="update-check"
            label="Look for updates when Kite Pack opens"
            hint="It asks GitHub for the newest release."
            on={updatePrefs.check}
            onToggle={() => onUpdatePrefs({ ...updatePrefs, check: !updatePrefs.check })}
          />
          <Switch
            id="update-auto"
            label="Install updates by themselves"
            hint="They download and install in the background. Kite Pack still asks before it restarts."
            on={updatePrefs.auto}
            onToggle={() => onUpdatePrefs({ ...updatePrefs, auto: !updatePrefs.auto })}
          />
          <div className="option-row">
            <button className="small-button" onClick={() => void checkNow()}>
              Check now
            </button>
            {checkNote && (
              <span className="field-hint" role="status">
                {checkNote}
              </span>
            )}
          </div>
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
            <div>
              <dt>ShadowMount+</dt>
              <dd>
                {smp
                  ? `${smp.version} at ${smp.host}:${smp.port}${smp.installed ? "" : smp.baseInstalled ? ", base mounted" : ""}`
                  : smpError
                    ? `Not answering (${smpError})`
                    : "Not checked yet"}
              </dd>
            </div>
          </dl>
          <p className="field-hint">The source and the release notes are at github.com/nankhor/kite-pack.</p>
        </div>
      </div>
    </section>
  );
}
