import { createRoot } from "react-dom/client";
import App from "./App";
import { tauriBackend } from "./backend";
import { applyTheme, readTheme } from "./theme";
import "./styles.css";
import "./screen.css";

// Before the first paint, so the window never flashes the other theme.
applyTheme(readTheme());

const root = createRoot(document.getElementById("root")!);
const params = new URLSearchParams(location.search);
// The dev server's stand-in for the app, to look at the pages in a browser (demo.ts).
if (import.meta.env.DEV && params.has("demo")) {
  void import("./demo").then(({ demoBackend }) => root.render(<App backend={demoBackend(params)} />));
} else {
  root.render(<App backend={tauriBackend} />);
}
