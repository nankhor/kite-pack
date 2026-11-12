import { createRoot } from "react-dom/client";
import App from "./App";
import { tauriBackend } from "./backend";
import "./styles.css";
import "./screen.css";

createRoot(document.getElementById("root")!).render(<App backend={tauriBackend} />);
