import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import type { Backend } from "./backend";
import { footerNote, historyEntry, jobBar, packBadge } from "./model";
import { Dlc } from "./parts/Dlc";
import { Empty } from "./parts/Empty";
import { Footer } from "./parts/Footer";
import { Facts } from "./parts/Facts";
import { Header, tabId } from "./parts/Header";
import { HistoryPage } from "./parts/History";
import { JobBar } from "./parts/JobBar";
import { Library } from "./parts/Library";
import { Options } from "./parts/Options";
import { SettingsPage } from "./parts/Settings";
import { Showcase, type DetailsView } from "./parts/Showcase";
import { baseName, parentOf } from "./paths";
import { initial, reduce } from "./state";
import type { BundleSelection, GameInfo, Settings, Status, View } from "./types";

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

/** A DLC the console already has: it is not packed in again. */
export function onConsole(label: string | null, installed: string[]): boolean {
  return label !== null && installed.some((l) => l.toLowerCase() === label.toLowerCase());
}

export default function App({ backend, now = Date.now }: { backend: Backend; now?: () => number }) {
  const [s, dispatch] = useReducer(reduce, initial);
  const current = useRef(s);
  current.current = s;
  const booted = useRef(false);
  const [version, setVersion] = useState<string | null>(null);
  const [dlcEmu, setDlcEmu] = useState<string | null>(null);
  /** Reading the picks against the game. */
  const [readingPicks, setReadingPicks] = useState(false);
  /** The game's story and pictures from IGDB, for the game whose key it carries. */
  const [details, setDetails] = useState<{ key: string; view: DetailsView }>({ key: "", view: { state: "off" } });
  const [detailsOn, setDetailsOnState] = useState(true);
  /** Bumped to look the game up again: Try again, or a game picked with Wrong game?. */
  const [lookup, setLookup] = useState(0);
  const gameTitle = s.game?.title ?? null;
  const gameTitleId = s.game?.titleId ?? null;
  const detailsKey = gameTitleId ?? gameTitle ?? "";

  useEffect(() => {
    let live = true;
    void backend.detailsOn().then((on) => live && setDetailsOnState(on));
    return () => {
      live = false;
    };
  }, [backend]);

  useEffect(() => {
    if (!gameTitle || !detailsOn) {
      setDetails({ key: detailsKey, view: { state: "off" } });
      return;
    }
    let live = true;
    setDetails({ key: detailsKey, view: { state: "loading" } });
    backend.gameDetails(gameTitle, gameTitleId).then(
      (d) => live && setDetails({ key: detailsKey, view: d ? { state: "found", details: d } : { state: "none" } }),
      () => live && setDetails({ key: detailsKey, view: { state: "offline" } }),
    );
    return () => {
      live = false;
    };
  }, [backend, gameTitle, gameTitleId, detailsKey, detailsOn, lookup]);

  async function pickGame(id: number | null) {
    if (!gameTitle) return;
    await backend.pickGame(gameTitle, gameTitleId, id);
    setLookup((n) => n + 1);
  }

  function switchDetails(on: boolean) {
    setDetailsOnState(on);
    void backend.setDetailsOn(on);
  }
  /** The PS5 tab's installed games, read only when asked. */
  // Jobs already written to History, so each goes in once.
  const recorded = useRef(new Set<string>());
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
    void backend.dlcEmuVersion().then(setDlcEmu).catch(() => {});
    void backend
      .appVersion()
      .then(setVersion)
      .catch(() => {});
  }, [backend]);

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

  // What the picked DLC and backport would add, read again whenever the picks or the game change.
  const picks = s.bundle;
  const source = s.source;
  useEffect(() => {
    if (!source) return;
    let stale = false;
    setReadingPicks(true);
    backend
      .bundlePlan(source, picks.dlc, picks.backport)
      .then((plan) => {
        if (!stale) dispatch({ type: "plan", plan });
      })
      .catch(() => {
        if (!stale) dispatch({ type: "plan", plan: null });
      })
      .finally(() => {
        if (!stale) setReadingPicks(false);
      });
    return () => {
      stale = true;
    };
  }, [backend, source, picks]);

  // A finished job goes into History, once; DLC packed in counts with it.
  useEffect(() => {
    if (s.phase !== "done" || !s.job || !s.game || recorded.current.has(s.job.id)) return;
    recorded.current.add(s.job.id);
    const bundled = (s.bundlePlan?.items ?? []).filter((i) => i.kind === "dlc" && i.route === "inPack" && !i.problem).length;
    void backend.addHistory(historyEntry(s.job, s.game, now(), bundled)).catch(() => {});
  }, [s.phase, s.job, s.game, s.bundlePlan, backend, now]);

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
        void loadOnPs5(game.titleId);
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
  const installedLabels = s.onPs5?.dlc ?? [];

  async function pack() {
    if (!s.source) return;
    dispatch({ type: "starting" });
    const send = s.options.send && sendable;
    try {
      // Only what goes inside the packed copy is sent; DLC the console already has stays out.
      const items = (s.bundlePlan?.items ?? []).filter(
        (i) => i.route === "inPack" && !i.problem && !onConsole(i.label, installedLabels),
      );
      const dlc = items.filter((i) => i.kind === "dlc").map((i) => i.path);
      const backport = items.find((i) => i.kind === "backport")?.path ?? null;
      const bundle: BundleSelection | undefined = dlc.length || backport ? { dlc, backport } : undefined;
      const options = {
        ...s.options,
        send,
        deletePacked: send && s.options.deletePacked,
        dlc: [],
        bundle,
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

  async function changeOutput() {
    const dir = await backend.pickFolder();
    if (!dir) return;
    dispatch({ type: "options", patch: { outputDir: dir } });
    await saveSettings({ packedFolder: dir });
  }

  /** Appends what was picked to the packed-copy selection, without duplicates. */
  function addPicks(paths: string[]) {
    const have = current.current.bundle.dlc;
    const fresh = paths.filter((p) => !have.includes(p));
    if (fresh.length === 0) return;
    dispatch({ type: "bundle", bundle: { ...current.current.bundle, dlc: [...have, ...fresh] } });
  }

  async function addDlcPackages() {
    addPicks(await backend.pickDlcFiles());
  }

  async function addDlcFolder() {
    const dir = await backend.pickDlcFolder();
    if (dir) addPicks([dir]);
  }

  async function addBackport() {
    const dir = await backend.pickBackport();
    if (!dir) return;
    dispatch({ type: "bundle", bundle: { ...current.current.bundle, backport: dir } });
  }

  function removePick(path: string) {
    const bundle = current.current.bundle;
    dispatch({
      type: "bundle",
      bundle: { dlc: bundle.dlc.filter((p) => p !== path), backport: bundle.backport === path ? null : bundle.backport },
    });
  }

  const showView = useCallback(
    (view: View) => {
      dispatch({ type: "view", view });
      if (view === "ps5" || view === "settings") void refreshStatus();
    },
    // refreshStatus reads the backend only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [backend],
  );
  const loadHistory = useCallback(() => backend.history(), [backend]);

  const header = <Header view={s.view} status={s.status} badge={packBadge(s.job, s.progress)} onView={showView} />;
  const panel = (view: View) => ({ role: "tabpanel", "aria-labelledby": tabId(view) }) as const;

  if (s.view === "ps5" && s.settings) {
    return (
      <div className="screen">
        {header}
        <div className="page" {...panel("ps5")}>
          {s.settings.address ? (
            <Library
              backend={backend}
              destination={s.settings.destination}
              statusNote={
                s.status?.state === "notReachable" ? "The PS5 isn't reachable; check the Kite helper is running." : null
              }
            />
          ) : (
            <section className="library" aria-label="Games on the PS5">
              <div className="library-top">
                <div className="library-heading">
                  <h1 className="library-title">On the PS5</h1>
                  <p className="library-sub">No PS5 set up yet. Choose yours in Settings, and its games show up here.</p>
                </div>
                <div className="buttons">
                  <button className="button secondary" onClick={() => showView("settings")}>
                    Open Settings
                  </button>
                </div>
              </div>
            </section>
          )}
        </div>
      </div>
    );
  }

  if (s.view === "history") {
    return (
      <div className="screen">
        {header}
        <div className="page" {...panel("history")}>
          <HistoryPage
            load={loadHistory}
            onClear={() => backend.clearHistory()}
            onShow={(path) => void backend.reveal(path)}
            now={now}
          />
        </div>
      </div>
    );
  }

  if (s.view === "settings" && s.settings) {
    return (
      <div className="screen">
        {header}
        <div className="page">
          <SettingsPage
            settings={s.settings}
            status={s.status}
            version={version}
            dlcEmuVersion={dlcEmu}
            detailsOn={detailsOn}
            onDetailsOn={switchDetails}
            onSave={saveSettings}
            onPickFolder={() => backend.pickFolder()}
            onDiscover={() => backend.discover()}
            onStartHelper={async () => {
              const outcome = await backend.startHelper();
              // The helper takes a few seconds to start on the PS5.
              setTimeout(() => void refreshStatus(), 4000);
              return outcome;
            }}
          />
        </div>
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
          onSeeLibrary={() => showView("ps5")}
        />
      </div>
    );
  }

  const done = s.phase === "done";
  const sent = !!s.job?.steps.some((st) => st.kind === "send" && st.state === "done");
  const packedKept = !!s.job?.packed && !s.job.steps.some((st) => st.kind === "deletePacked" && st.state === "done");
  const sending = s.options.send && sendable;
  const goingIn = (s.bundlePlan?.items ?? []).filter(
    (i) => i.route === "inPack" && !i.problem && !onConsole(i.label, installedLabels),
  ).length;
  const note =
    s.phase === "ready" || !s.job
      ? footerNote({ kind: "ready", outputDir: s.options.outputDir, send: sending, bundled: goingIn })
      : footerNote({ kind: s.phase, job: s.job });

  const view = details.key === detailsKey ? details.view : { state: "loading" as const };

  return (
    <div className={s.dragging ? "screen pack-screen dragging" : "screen pack-screen"}>
      {header}
      <div className="pack-page">
        <div className="pack-main">
          <Showcase
            game={s.game}
            source={s.source ?? ""}
            done={done}
            onPs5={s.onPs5}
            headline={sent ? "On the PS5" : "Packed"}
            view={view}
            onSearch={(q) => backend.searchGames(q)}
            onPick={(id) => void pickGame(id)}
            onRetry={() => setLookup((n) => n + 1)}
          />
          {s.message && (
            <p role="alert" className="message">
              {s.message}
            </p>
          )}
        </div>
        <aside className="pack-panel" aria-label="Pack and send">
          <div className="panel-body">
            <Facts game={s.game} job={s.job} done={done} onPs5={s.onPs5} />
            {s.phase === "ready" ? (
              <Options
                options={s.options}
                status={s.status}
                destination={s.settings?.destination ?? "/data/homebrew"}
                onChange={(patch) => dispatch({ type: "options", patch })}
                onChangeFolder={() => void changeOutput()}
              />
            ) : (
              s.job && <JobBar bar={jobBar(s.job, s.progress, now(), { inside: goingIn })} />
            )}
            {s.phase === "ready" && (
              <Dlc
                plan={s.bundlePlan}
                busy={readingPicks}
                installed={installedLabels}
                gameIcon={s.game.iconDataUrl}
                onAddPackages={() => void addDlcPackages()}
                onAddFolder={() => void addDlcFolder()}
                onAddBackport={() => void addBackport()}
                onRemove={removePick}
                onRemoveAll={() => dispatch({ type: "bundle", bundle: { dlc: [], backport: null } })}
              />
            )}
          </div>
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
        </aside>
      </div>
    </div>
  );
}
