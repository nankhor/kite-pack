// Light or dark: the computer's choice unless Settings picks one. `<html class="dark">` switches
// every token in styles.css.

export type ThemeChoice = "system" | "light" | "dark";

const KEY = "kite.theme";

type Store = Pick<Storage, "getItem" | "setItem">;

export function readTheme(store: Store = localStorage): ThemeChoice {
  try {
    const value = store.getItem(KEY);
    return value === "light" || value === "dark" ? value : "system";
  } catch {
    return "system";
  }
}

export function writeTheme(choice: ThemeChoice, store: Store = localStorage): void {
  try {
    store.setItem(KEY, choice);
  } catch {
    // Storage can be unavailable; the choice then lasts until the app closes.
  }
}

/** The computer's dark-mode setting, where the window can read it. */
export function systemDark(): MediaQueryList | null {
  return typeof window !== "undefined" && window.matchMedia ? window.matchMedia("(prefers-color-scheme: dark)") : null;
}

/** Applies `choice` now and, for System, each time the computer switches. Returns a stop. */
export function applyTheme(choice: ThemeChoice, media: MediaQueryList | null = systemDark()): () => void {
  const root = document.documentElement;
  const set = () => root.classList.toggle("dark", choice === "dark" || (choice === "system" && !!media?.matches));
  set();
  if (choice !== "system" || !media) return () => {};
  media.addEventListener("change", set);
  return () => media.removeEventListener("change", set);
}
