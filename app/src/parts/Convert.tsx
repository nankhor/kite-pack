import type { ConvertChoice, Status } from "../types";
import { Switch } from "./Options";

interface Props {
  choice: ConvertChoice;
  /** How the PS5 stands; decides whether Install on PS5 can be switched on. */
  status: Status | null;
  onChange: (patch: Partial<ConvertChoice>) => void;
  onChangeFolder: () => void;
}

/** Install on PS5's hint, and whether it can be switched on, for each PS5 state. */
export function installState(status: Status | null): [boolean, string] {
  switch (status?.state) {
    case "running":
      return [true, "Streams it to the PS5 as soon as it's ready"];
    case "notReachable":
      return [true, "The PS5 isn't reachable right now. Installing waits for it."];
    case "refused":
      return [false, status.reason];
    default:
      return [false, "Set up your PS5 in Settings first."];
  }
}

/** Before Convert: the package, and what happens once it's written. */
export function ConvertOptions({ choice, status, onChange, onChangeFolder }: Props) {
  const [canInstall, installHint] = installState(status);
  const installing = canInstall && choice.install;
  return (
    <section className="options" aria-label="Options">
      <section className="column" aria-labelledby="convert-package">
        <div className="column-head">
          <h2 id="convert-package">Fake package</h2>
          <p className="column-sub">The game as one .fpkg, ready to install</p>
        </div>
        <div className="option-row">
          <span className="option-label">Compression</span>
          <div className="pills" role="group" aria-label="Compression">
            {([false, true] as const).map((on) => (
              <button
                key={String(on)}
                aria-pressed={choice.compress === on}
                className={choice.compress === on ? "pill on" : "pill"}
                onClick={() => onChange({ compress: on })}
              >
                {on ? "On" : "Off"}
              </button>
            ))}
          </div>
          <span className="option-hint">
            {choice.compress ? "Smaller, but only proven to install on firmware 5.10" : "Off installs on every firmware"}
          </span>
        </div>
        <div className="option-row">
          <span className="option-label">Saves to</span>
          <span className="path" title={choice.outputDir}>
            {choice.outputDir}
          </span>
          <button className="small-button" onClick={onChangeFolder}>
            Change
          </button>
        </div>
      </section>
      <section className="column" aria-labelledby="convert-then">
        <div className="column-head">
          <h2 id="convert-then">Then</h2>
          <p className="column-sub">What happens once the .fpkg is written</p>
        </div>
        <Switch
          id="convert-install"
          label="Install on PS5"
          hint={installHint}
          on={installing}
          disabled={!canInstall}
          onToggle={() => onChange({ install: !choice.install })}
        />
        {installing && (
          <Switch
            id="convert-delete"
            label="Delete the .fpkg after"
            hint="Once the PS5 confirms the install"
            on={choice.deleteAfter}
            onToggle={() => onChange({ deleteAfter: !choice.deleteAfter })}
          />
        )}
        <p className="convert-note">Needs kstuff with fake-package support running on the PS5.</p>
      </section>
    </section>
  );
}
