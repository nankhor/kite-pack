/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri serves the built files; the dev server is only for `tauri dev`.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: ["es2022", "safari16"], outDir: "dist" },
  test: { environment: "jsdom", setupFiles: ["./src/test-setup.ts"] },
});
