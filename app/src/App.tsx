import { useEffect, useReducer, useRef } from "react";
import type { Backend } from "./backend";
import { footerNote, stepRows } from "./model";
import { Empty } from "./parts/Empty";
import { Footer } from "./parts/Footer";
import { Game } from "./parts/Game";
import { Header } from "./parts/Header";
import { Options } from "./parts/Options";
import { Steps } from "./parts/Steps";
import { baseName, parentOf } from "./paths";
import { initial, reduce } from "./state";
import type { GameInfo } from "./types";

const OUTPUT_KEY = "kite.outputDir";

function storedOutputDir(): string | null {
  try {
    return localStorage.getItem(OUTPUT_KEY);
  } catch {
    return null;
  }
}

function storeOutputDir(dir: string) {
  try {
    localStorage.setItem(OUTPUT_KEY, dir);
  } catch {
    // A webview without storage just forgets the folder next time.
  }
}

/** What to show for a job whose game can no longer be read. */
function stand_in(source: string): GameInfo {
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

export default function App({ backend }: { backend: Backend }) {
  const [s, dispatch] = useReducer(reduce, initial);
  const current = useRef(s);
  current.current = s;
  const booted = useRef(false);

  async function open(path: string) {
    if (current.current.phase === "running") {
      dispatch({ type: "message", text: "A game is already being packed." });
      return;
    }
    try {
      const game = await backend.inspect(path);
      dispatch({ type: "game", game, source: path, outputDir: storedOutputDir() ?? parentOf(path) });
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
    return () => {
      stopEvents();
      stopDrops();
    };
    // `open` reads the latest state through `current`; the listeners stay for the app's life.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [backend]);

  useEffect(() => {
    if (booted.current) return;
    booted.current = true;
    void (async () => {
      const left = await backend.unfinished();
      if (left) {
        const game = await backend.inspect(left.source).catch(() => stand_in(left.source));
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

  async function pack() {
    if (!s.source) return;
    try {
      dispatch({ type: "started", job: await backend.start(s.source, s.options) });
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
    storeOutputDir(dir);
    dispatch({ type: "options", patch: { outputDir: dir } });
  }

  if (s.phase === "empty" || !s.game) {
    return (
      <div className={s.dragging ? "screen dragging" : "screen"}>
        <Header />
        <Empty
          message={s.message}
          onChooseFolder={() => void choose(backend.pickFolder)}
          onChooseFile={() => void choose(backend.pickFile)}
        />
      </div>
    );
  }

  const done = s.phase === "done";
  const note =
    s.phase === "ready" || !s.job
      ? footerNote({ kind: "ready", outputDir: s.options.outputDir })
      : footerNote({ kind: s.phase, job: s.job });

  return (
    <div className={s.dragging ? "screen dragging" : "screen"}>
      <Header />
      <div className="spacer" />
      <Game game={s.game} source={s.source ?? ""} job={s.job} done={done} />
      {s.message && (
        <p role="alert" className="message">
          {s.message}
        </p>
      )}
      <div className="spacer" />
      {s.phase === "ready" ? (
        <Options
          options={s.options}
          onChange={(patch) => dispatch({ type: "options", patch })}
          onChangeFolder={() => void changeOutput()}
        />
      ) : (
        s.job && <Steps rows={stepRows(s.job, s.progress, Date.now())} />
      )}
      <Footer
        phase={s.phase}
        note={note}
        canShow={!!s.job?.packed}
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
