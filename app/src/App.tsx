import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import type { Backend } from "./backend";
import { footerNote, historyEntry, packBadge, stepRows } from "./model";
import { Dlc } from "./parts/Dlc";
import { Empty } from "./parts/Empty";
import { Footer } from "./parts/Footer";
import { Game } from "./parts/Game";
import { Header, tabId } from "./parts/Header";
import { HistoryPage } from "./parts/History";
import { Options } from "./parts/Options";
import { Ps5Page } from "./parts/Ps5";
import { SettingsPage } from "./parts/Settings";
import { Steps } from "./parts/Steps";
import { baseName, parentOf } from "./paths";
import { initial, reduce } from "./state";
import type { GameInfo, PickedDlc, Settings, Status, View } from "./types";

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

/** The names of picked DLC by path, for the steps. */
function dlcNames(dlc: PickedDlc[]): Record<string, string> {
  return Object.fromEntries(dlc.map((d) => [d.path, d.info.name ?? d.info.label]));
}

/** "a.pkg, b.pkg and 3 more". */
function fileList(names: string[]): string {
  const shown = names.slice(0, 3).join(", ");
  return names.length > 3 ? `${shown} and ${names.length - 3} more` : shown;
}

export default function App({ backend, now = Date.now }: { backend: Backend; now?: () => number }) {
  const [s, dispatch] = useReducer(reduce, initial);
  const current = useRef(s);
  current.current = s;
  const booted = useRef(false);
  const [version, setVersion] = useState<string | null>(null);
  const [checkingDlc, setCheckingDlc] = useState(false);
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

  async function open(path: string) {
    if (current.current.phase === "running") {
      dispatch({ type: "message", text: "A game is already being packed." });
      return;
    }
    try {
      const [game, settings] = await Promise.all([backend.inspect(path), loadSettings()]);
      dispatch({ type: "game", game, source: path, outputDir: settings.packedFolder ?? parentOf(path) });
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
    void backend
      .appVersion()
      .then(setVersion)
      .catch(() => {});
  }, [backend]);

  // A finished job goes into History, once.
  useEffect(() => {
    if (s.phase !== "done" || !s.job || !s.game || recorded.current.has(s.job.id)) return;
    recorded.current.add(s.job.id);
    void backend.addHistory(historyEntry(s.job, s.game, now())).catch(() => {});
  }, [s.phase, s.job, s.game, backend, now]);

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
        send,
        deletePacked: send && s.options.deletePacked,
        dlc: send ? s.dlc.map((d) => d.path) : [],
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

  /** Adds picked DLC, skipping ones already there, and says which weren't DLC for this game. */
  function addDlc(found: PickedDlc[], refused: string[]) {
    const have = current.current.dlc;
    const fresh = found.filter((d) => !have.some((h) => h.path === d.path));
    dispatch({ type: "dlc", dlc: [...have, ...fresh] });
    dispatch({ type: "message", text: refused.length ? `Not DLC for this game: ${fileList(refused)}.` : null });
  }

  async function addDlcFiles() {
    const titleId = current.current.game?.titleId;
    if (!titleId) return;
    const paths = (await backend.pickDlcFiles()).filter((p) => !current.current.dlc.some((d) => d.path === p));
    if (paths.length === 0) return;
    setCheckingDlc(true);
    const checked = await Promise.allSettled(paths.map((path) => backend.checkDlc(path, titleId)));
    setCheckingDlc(false);
    const found: PickedDlc[] = [];
    const refused: string[] = [];
    checked.forEach((c, i) => {
      if (c.status === "fulfilled") found.push({ path: paths[i], info: c.value });
      else refused.push(baseName(paths[i]));
    });
    addDlc(found, refused);
  }

  async function addDlcFolder() {
    const titleId = current.current.game?.titleId;
    if (!titleId) return;
    const dir = await backend.pickFolder();
    if (!dir) return;
    setCheckingDlc(true);
    try {
      const folder = await backend.dlcInFolder(dir, titleId);
      if (folder.found.length === 0) {
        dispatch({ type: "message", text: "No DLC for this game in that folder." });
        return;
      }
      addDlc(folder.found, folder.skipped);
    } catch (e) {
      dispatch({ type: "message", text: String(e) });
    } finally {
      setCheckingDlc(false);
    }
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
          <Ps5Page
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
          />
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
            onSave={saveSettings}
            onPickFolder={() => backend.pickFolder()}
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
        />
      </div>
    );
  }

  const done = s.phase === "done";
  const sent = !!s.job?.steps.some((st) => st.kind === "send" && st.state === "done");
  const packedKept =
    !!s.job?.packed && !s.job.steps.some((st) => st.kind === "deletePacked" && st.state === "done");
  const sending = s.options.send && sendable;
  const note =
    s.phase === "ready" || !s.job
      ? footerNote({ kind: "ready", outputDir: s.options.outputDir, send: sending, dlc: s.dlc.length })
      : footerNote({ kind: s.phase, job: s.job });
  let dlcBlocked: string | null = null;
  if (!s.game.titleId) dlcBlocked = "This game has no title ID, so its DLC can't be matched.";
  else if (!sending) dlcBlocked = "Turn on Send to PS5 to install DLC.";

  return (
    <div className={s.dragging ? "screen dragging" : "screen"}>
      {header}
      <div className="spacer" />
      <Game game={s.game} source={s.source ?? ""} job={s.job} done={done} headline={sent ? "On the PS5" : "Packed"} />
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
        s.job && <Steps rows={stepRows(s.job, s.progress, now(), dlcNames(s.dlc))} />
      )}
      {s.phase === "ready" && (
        <Dlc
          picked={s.dlc}
          blocked={dlcBlocked}
          busy={checkingDlc}
          gameIcon={s.game.iconDataUrl}
          onAddFiles={() => void addDlcFiles()}
          onAddFolder={() => void addDlcFolder()}
          onRemove={(path) => dispatch({ type: "dlc", dlc: s.dlc.filter((d) => d.path !== path) })}
          onRemoveAll={() => dispatch({ type: "dlc", dlc: [] })}
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
