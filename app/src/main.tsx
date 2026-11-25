import { createRoot } from "react-dom/client";
import App from "./App";
import { tauriBackend } from "./backend";
import { applyTheme, readTheme } from "./theme";
import "./styles.css";
import "./screen.css";

// Before the first paint, so the window never flashes the other theme.
applyTheme(readTheme());

createRoot(document.getElementById("root")!).render(<App backend={tauriBackend} />);
