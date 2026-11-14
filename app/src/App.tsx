import { useEffect, useReducer, useRef } from "react";
import type { Backend } from "./backend";
import { footerNote, stepRows } from "./model";
import { Empty } from "./parts/Empty";
import { Footer } from "./parts/Footer";
import { Game } from "./parts/Game";
import { Bundle } from "./parts/Bundle";
import { Header } from "./parts/Header";
import { Library } from "./parts/Library";
import { Options } from "./parts/Options";
import { SettingsPage } from "./parts/Settings";
import { Steps } from "./parts/Steps";
import { baseName, parentOf } from "./paths";
import { initial, reduce } from "./state";
import type { GameInfo, Settings, Status } from "./types";

/** How often the PS5 chip checks the helper while the app is open. */
const STATUS_EVERY_MS = 10_000;

/** What to show for a job whose game can no longer be read. */
function standIn(source: string): GameInfo {
  return {
    kind: "folder",
    title: baseName(source),
    titleId: null,
    contentId: null,
    version: null,
    totalBytes: 0,
    dlcs: [],
    iconDataUrl: null,
    findings: [],
  };
}

/** Whether Send can be switched on: a PS5 is chosen and its helper doesn't refuse us. */
export function canSend(status: Status | null): boolean {
  return status?.state === "running" || status?.state === "notReachable";
}

export default function App({ backend }: { backend: Backend }) {
  const [s, dispatch] = useReducer(reduce, initial);
  const current = useRef(s);
  current.current = s;
  const booted = useRef(false);
  // Settings load once; anything that needs them waits on this.
  const settingsReady = useRef<Promise<Settings> | null>(null);

  function loadSettings(): Promise<Settings> {
    if (!settingsReady.current) {
      settingsReady.current = backend.settings().then((settings) => {
        dispatch({ type: "settings", settings });
        return settings;
      });
    }
    return settingsReady.current;
  }

  async function refreshStatus() {
    try {
      dispatch({ type: "status", status: await backend.ps5Status() });
    } catch {
      dispatch({ type: "status", status: { state: "notReachable" } });
    }
  }

  async function saveSettings(patch: Partial<Settings>) {
    const base = current.current.settings ?? (await loadSettings());
    const next = { ...base, ...patch };
    await backend.saveSettings(next);
    dispatch({ type: "settings", settings: next });
    if ("address" in patch || "helperPort" in patch) await refreshStatus();
  }

  /** What the console already has of this game, for the screen's "already on the PS5". Silent
   * when the console cannot be read: the game can still be packed and sent. */
  async function loadOnPs5(titleId: string | null) {
    if (!titleId) {
      dispatch({ type: "onPs5", game: null });
      return;
    }
    try {
      dispatch({ type: "onPs5", game: await backend.installedTitle(titleId) });
    } catch {
      dispatch({ type: "onPs5", game: null });
    }
  }

  async function open(path: string) {
    if (current.current.phase === "running") {
      dispatch({ type: "message", text: "A game is already being packed." });
      return;
    }
    try {
      const [game, settings] = await Promise.all([backend.inspect(path), loadSettings()]);
      dispatch({ type: "game", game, source: path, outputDir: settings.packedFolder ?? parentOf(path) });
      void loadOnPs5(game.titleId);
    } catch (e) {
      dispatch({ type: "message", text: String(e) });
    }
  }

  async function choose(pick: () => Promise<string | null>) {
    const path = await pick();
    if (path) await open(path);
  }

  useEffect(() => {
    const stopEvents = backend.onEvent((event) => dispatch({ type: "event", event, now: Date.now() }));
    const stopDrops = backend.onDrop((d) => {
      if (d.type === "over") dispatch({ type: "drag", on: true });
      else if (d.type === "leave") dispatch({ type: "drag", on: false });
      else {
        dispatch({ type: "drag", on: false });
        if (d.paths.length !== 1) dispatch({ type: "message", text: "Drop one game at a time." });
        else void open(d.paths[0]);
      }
    });
    const timer = setInterval(() => void refreshStatus(), STATUS_EVERY_MS);
    return () => {
      stopEvents();
      stopDrops();
      clearInterval(timer);
    };
    // `open` and `refreshStatus` read the latest state through refs; the listeners stay for the app's life.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [backend]);

  useEffect(() => {
    if (booted.current) return;
    booted.current = true;
    // The PS5: with none saved, take the one helper that answers on the network.
    void (async () => {
      const settings = await loadSettings();
      if (!settings.address) {
        const found = await backend.discover().catch(() => []);
        if (found.length === 1) {
          await saveSettings({ address: found[0].address, helperPort: found[0].port });
          return;
        }
      }
      await refreshStatus();
    })();
    // A job left by a closed app, or a game given at launch.
    void (async () => {
      const left = await backend.unfinished();
      if (left) {
        const game = await backend.inspect(left.source).catch(() => standIn(left.source));
        dispatch({ type: "restored", job: left, game });
        if (left.state !== "failed") {
          try {
            dispatch({ type: "started", job: await backend.carryOn(left.id) });
          } catch (e) {
            dispatch({ type: "message", text: String(e) });
          }
        }
        return;
      }
      const path = await backend.initialPath();
      if (path) await open(path);
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [backend]);

  const sendable = canSend(s.status);

  async function pack() {
    if (!s.source) return;
    dispatch({ type: "starting" });
    const send = s.options.send && sendable;
    try {
      const options = {
        ...s.options,
        bundle: s.bundle,
        send,
        deletePacked: send && s.options.deletePacked,
      };
      dispatch({ type: "started", job: await backend.start(s.source, options) });
    } catch (e) {
      dispatch({ type: "message", text: String(e) });
    }
  }

  async function retry() {
    if (!s.job) return;
    try {
      dispatch({ type: "started", job: await backend.carryOn(s.job.id) });
    } catch (e) {
      dispatch({ type: "message", text: String(e) });
    }
  }

  async function discard() {
    if (!s.job) return;
    await backend.discard(s.job.id).catch(() => {});
    dispatch({ type: "back" });
  }

  /** Adds one picked DLC (a folder or a package) to what the pack will carry. */
  async function addDlc(pick: () => Promise<string | null>) {
    const path = await pick();
    if (!path) return;
    const bundle = current.current.bundle;
    if (bundle.dlc.includes(path)) return;
    dispatch({ type: "bundle", bundle: { ...bundle, dlc: [...bundle.dlc, path] } });
  }

  async function addBackport() {
    const path = await backend.pickBackport();
    if (!path) return;
    dispatch({ type: "bundle", bundle: { ...current.current.bundle, backport: path } });
  }

  async function changeOutput() {
    const dir = await backend.pickFolder();
    if (!dir) return;
    dispatch({ type: "options", patch: { outputDir: dir } });
    await saveSettings({ packedFolder: dir });
  }

  // Whether what was picked can go into the pack; read whenever the picks (or the game) change.
  const bundle = s.bundle;
  const source = s.source;
  useEffect(() => {
    if (!source) return;
    let stale = false;
    backend
      .bundlePlan(source, bundle.dlc, bundle.backport)
      .then((plan) => {
        if (!stale) dispatch({ type: "plan", plan });
      })
      .catch(() => {
        if (!stale) dispatch({ type: "plan", plan: null });
      });
    return () => {
      stale = true;
    };
  }, [backend, source, bundle]);

  const header = (
    <Header
      status={s.status}
      onOpenSettings={() => {
        dispatch({ type: "view", view: "settings" });
        void refreshStatus();
      }}
    />
  );

  if (s.view === "settings" && s.settings) {
    return (
      <div className="screen">
        {header}
        <SettingsPage
          settings={s.settings}
          status={s.status}
          onSave={saveSettings}
          onDiscover={() => backend.discover()}
          onStartHelper={async () => {
            const outcome = await backend.startHelper();
            // The helper takes a few seconds to start on the PS5.
            setTimeout(() => void refreshStatus(), 4000);
            return outcome;
          }}
          onPickFolder={() => backend.pickFolder()}
          onDone={() => dispatch({ type: "view", view: "main" })}
        />
      </div>
    );
  }

  if (s.view === "library") {
    return (
      <div className="screen">
        {header}
        <Library
          backend={backend}
          destination={s.settings?.destination ?? "/data/homebrew"}
          statusNote={s.status?.state === "notReachable" ? "The PS5 isn't reachable; check the Kite helper is running." : null}
          onBack={() => dispatch({ type: "view", view: "main" })}
        />
      </div>
    );
  }

  if (s.phase === "empty" || !s.game) {
    return (
      <div className={s.dragging ? "screen dragging" : "screen"}>
        {header}
        <Empty
          message={s.message}
          onChooseFolder={() => void choose(backend.pickFolder)}
          onChooseFile={() => void choose(backend.pickFile)}
          onSeeLibrary={() => dispatch({ type: "view", view: "library" })}
        />
      </div>
    );
  }

  const done = s.phase === "done";
  const sent = !!s.job?.steps.some((st) => st.kind === "send" && st.state === "done");
  const packedKept =
    !!s.job?.packed && !s.job.steps.some((st) => st.kind === "deletePacked" && st.state === "done");
  const note =
    s.phase === "ready" || !s.job
      ? footerNote({ kind: "ready", outputDir: s.options.outputDir, send: s.options.send && sendable })
      : footerNote({ kind: s.phase, job: s.job });

  return (
    <div className={s.dragging ? "screen dragging" : "screen"}>
      {header}
      <div className="spacer" />
      <Game
        game={s.game}
        source={s.source ?? ""}
        job={s.job}
        done={done}
        onPs5={s.onPs5}
        headline={sent ? "On the PS5" : "Packed"}
      />
      {s.message && (
        <p role="alert" className="message">
          {s.message}
        </p>
      )}
      <div className="spacer" />
      {s.phase === "ready" ? (
        <Options
          options={s.options}
          status={s.status}
          destination={s.settings?.destination ?? "/data/homebrew"}
          onChange={(patch) => dispatch({ type: "options", patch })}
          onChangeFolder={() => void changeOutput()}
        />
      ) : (
        s.job && <Steps rows={stepRows(s.job, s.progress, Date.now())} />
      )}
      {s.phase === "ready" && (
        <Bundle
          selection={s.bundle}
          plan={s.bundlePlan}
          onAddFolder={() => void addDlc(backend.pickDlcFolder)}
          onAddPackage={() => void addDlc(backend.pickDlcPackage)}
          onAddBackport={() => void addBackport()}
          onRemove={(bundle) => dispatch({ type: "bundle", bundle })}
        />
      )}
      <Footer
        phase={s.phase}
        note={note}
        canShow={packedKept}
        onPack={() => void pack()}
        onChangeGame={() => dispatch({ type: "reset" })}
        onCancel={() => void (s.phase === "failed" ? discard() : backend.cancel())}
        onRetry={() => void retry()}
        onShow={() => s.job?.packed && void backend.reveal(s.job.packed)}
        onAnother={() => dispatch({ type: "reset" })}
      />
    </div>
  );
}
