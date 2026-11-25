# New look, part 1: design language and layout — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Kite Pack's screen in the new dashboard design (light and dark), with a sidebar, a page of its own for installs, and every existing screen restyled.

**Architecture:** A token stylesheet (CSS variables, light on `:root`, dark on `.dark`), a theme module that sets `<html class="dark">` from the choice and the system, and a `Sidebar` that replaces the tab header. `App` keeps its one game/job state; the view decides the page, and a dropped item picks its page by kind.

**Tech Stack:** React 19, TypeScript, Vite, Vitest + Testing Library; Tauri 2 shell unchanged.

**Spec:** `docs/superpowers/specs/2026-11-25-new-look-design.md`. Visual reference: the "Kite Pack new look" canvas, page "Every page" (its `kite.css` is the token source below).

## Global Constraints

- Tokens, light: bg `#fafafa`, card `#ffffff`, line `#e4e4e7`, line2 `#d4d4d8`, fg `#09090b`, muted `#6b6b74`, soft `#f4f4f5`, primary `#18181b` on `#ffffff`, link `#2563eb`, ok `#15803d`/`#dcfce7`, warn `#b45309`/`#fef3c7`, fail `#b91c1c`/`#fee2e2`.
- Tokens, dark: bg `#09090b`, card `#111113`, line `#27272a`, line2 `#3f3f46`, fg `#fafafa`, muted `#a1a1aa`, soft `#1c1c1f`, primary `#fafafa` on `#09090b`, link `#8ab4ff`, ok `#4ade80`/`#052e16`, warn `#fbbf24`/`#3a2a05`, fail `#f87171`/`#3b0d0d`.
- Type: Geist (UI), Geist Mono (paths, ids), bundled from `@fontsource-variable/geist` and `@fontsource-variable/geist-mono`; page title 28 px/600, card title 15 px/600, body 14 px, notes 13 px.
- Shapes: cards 12 px radius, 1 px border, `0 1px 2px rgba(0,0,0,.04)`; controls 8 px; segmented 10 px outer.
- Sidebar items: Install ("Stream a .pkg to the PS5"), Pack ("Compress and send"), History; group "PS5" with its status dot and words; My games; Settings at the bottom. Convert, Files, Backups appear in later parts.
- The window never scrolls; each page scrolls inside the main area. Below 900 px wide the sidebar stacks on top.
- Copy rules: sentence case, plain verbs, no ALL CAPS labels; every message the current tests check stays unless the test changes with a reason.
- Anti-piracy line stays where it is today (Settings/About) if present.

## Review Focus

1. **Dark mode contrast:** muted text, badges and disabled controls stay at least 4.5:1 on their surfaces in both themes (`theme.test.ts` checks the token pairs).
2. **A narrow window (about 800 px):** nothing scrolls sideways; the sidebar stacks on top and the side column drops under the game card. Checked on the demo screenshots in Task 8.
3. **A running job when the person changes page:** the job keeps running, its sidebar item shows its badge, and coming back shows its progress (App test `a_running_job_keeps_its_place`).
4. **Dropping a package while a pack runs, or a dump while an install runs:** refused with today's "already being packed" style message, nothing replaced (App test `a_drop_while_a_job_runs_is_refused`).
5. **Theme set to System while the computer switches:** `<html>` follows without a restart (`theme.test.ts`).

---

### Task 1: Tokens, fonts and theme

**Files:**
- Modify: `app/package.json` (add the two Geist packages, drop `@fontsource/figtree`), `app/src/styles.css` (tokens, base, fonts), `app/src/main.tsx` (apply the theme before first render)
- Create: `app/src/theme.ts`
- Test: `app/src/theme.test.ts`

**Interfaces:**
- Produces: `type ThemeChoice = "system" | "light" | "dark"`; `readTheme(): ThemeChoice`; `writeTheme(c: ThemeChoice): void`; `applyTheme(c: ThemeChoice, media?: MediaQueryList): () => void` (sets/removes `dark` on `document.documentElement`, follows `media` changes for `system`, returns a stop function).

- [ ] Step 1: tests `follows_the_system_until_one_is_picked`, `follows_a_system_switch_live`, `remembers_the_choice` (localStorage key `kite.theme`), `tokens_read_well_in_both_themes` (contrast ≥ 4.5 for fg/muted/link/ok/warn/fail on card and bg, parsed from styles.css).
- [ ] Step 2: run `npx vitest run src/theme.test.ts` → fail (module missing).
- [ ] Step 3: implement; install the fonts.
- [ ] Step 4: run → pass; `npx vitest run src/css.test.ts` still passes.
- [ ] Step 5: commit "Screen: design tokens, Geist, light and dark".

### Task 2: Sidebar shell and routing

**Files:**
- Create: `app/src/parts/Sidebar.tsx`
- Modify: `app/src/types.ts` (`View = "install" | "pack" | "history" | "games" | "settings"`), `app/src/state.ts` (default view `pack`; `game` action takes the view), `app/src/App.tsx` (shell: sidebar + main; drop/open picks Install for a package, Pack otherwise), `app/src/screen.css` (shell layout), remove `app/src/parts/Header.tsx` once unused
- Test: `app/src/App.test.tsx`, `app/src/Ps5.test.tsx`

**Interfaces:**
- Consumes: `packBadge(job, progress)` from model.ts.
- Produces: `Sidebar({ view, status, badge, badgeOn: View | null, onView })`; nav items are links (`aria-current="page"` on the open one) inside `<nav aria-label="Sections">`; the PS5 group shows `chip(status)` words.

- [ ] Step 1: tests `the_sidebar_marks_the_page_you_are_on`, `a_package_opens_on_install`, `a_dump_opens_on_pack`, `a_running_job_keeps_its_place`, `a_drop_while_a_job_runs_is_refused`; update every existing test that finds a `tab` to find the sidebar link.
- [ ] Step 2: `npx vitest run` → the new ones fail.
- [ ] Step 3: implement.
- [ ] Step 4: `npx vitest run` and `npx tsc --noEmit -p .` → pass.
- [ ] Step 5: commit "Screen: sidebar, and a package opens on Install".

### Task 3: Install page

**Files:**
- Create: `app/src/parts/InstallPage.tsx`, `app/src/parts/GameCard.tsx` (the game card both pages use: art, icon, title, badges, story, details list, screenshots, from what `Showcase` shows today)
- Modify: `app/src/App.tsx`, `app/src/screen.css`, `app/src/model.ts` (an install's progress card words reuse `jobBar`)
- Test: `app/src/App.test.tsx`

- [ ] Step 1: tests (rework of today's package tests): `offers_install_for_a_package` (Install on PS5 → `backend.install(source)`), `a_ps4_package_only_installs` (no Pack offered on the Install page; its note), `shows_an_install_as_it_goes` ("12.4 GB of 45.6 GB on the PS5", Stop), `asks_before_closing_during_an_install` unchanged, `an_empty_install_page_asks_for_a_package` ("Drop a .pkg or .fpkg here", Choose file).
- [ ] Step 2–4: fail, implement, pass.
- [ ] Step 5: commit "Screen: an Install page".

### Task 4: Pack page

**Files:**
- Modify: `app/src/App.tsx`, `app/src/parts/Options.tsx`, `app/src/parts/Facts.tsx` (into the game card's details), `app/src/parts/Footer.tsx` (the main button and note in the side column; Change game in the top bar), `app/src/parts/JobBar.tsx` (in a card), `app/src/parts/Dlc.tsx` (in a card), `app/src/parts/Empty.tsx` (drop zone in the new look), `app/src/screen.css`
- Test: `app/src/App.test.tsx`

- [ ] Step 1: no new behaviour: run `npx vitest run` after each move; any test that changes says why in the commit.
- [ ] Step 2: restyle; the PS5 game package still offers Pack here.
- [ ] Step 3: `npx vitest run`, `npx tsc --noEmit -p .` → pass.
- [ ] Step 4: commit "Screen: Pack in the new look".

### Task 5: History

**Files:** Modify `app/src/parts/History.tsx`, `app/src/screen.css`. Test: `app/src/App.test.tsx` (history tests).

- [ ] Step 1: test `history_filters_by_what_was_done` (All / Install / Pack).
- [ ] Step 2–4: fail, implement (table: game, what, size, when, result; Show folder where a packed copy stayed; Clear history), pass.
- [ ] Step 5: commit "Screen: History as a table".

### Task 6: My games and a game's page

**Files:** Modify `app/src/parts/Library.tsx`, `app/src/parts/Ps5GamePage.tsx`, `app/src/parts/Ps5.tsx` if still used, `app/src/App.tsx` (free space per drive), `app/src/screen.css`. Test: `app/src/Ps5.test.tsx`.

- [ ] Step 1: test `my_games_shows_free_space_per_drive` (internal and, when present, the USB drive from `backend.freeSpace`), `my_games_searches_by_name`.
- [ ] Step 2–4: fail, implement (grid of cards; the game page uses GameCard plus Space on the PS5 and DLC cards), pass.
- [ ] Step 5: commit "Screen: My games and a game's page".

### Task 7: Settings and Appearance

**Files:** Modify `app/src/parts/Settings.tsx`, `app/src/screen.css`. Test: `app/src/App.test.tsx` (settings tests).

- [ ] Step 1: test `appearance_switches_the_theme` (System / Light / Dark → `<html>` class and `kite.theme`).
- [ ] Step 2–4: fail, implement (cards: PS5, This computer, Game details, Appearance, Updates), pass.
- [ ] Step 5: commit "Screen: Settings in cards, with Appearance".

### Task 8: Look at it

**Files:** Create `app/src/demo.ts` (a stand-in backend with made-up games, loaded only by `vite` in development with `?demo`), modify `app/src/main.tsx`.

- [ ] Step 1: run `npx vite --port 5199` and screenshot each page at 1440×900 and 820×900, light and dark, with headless Chrome.
- [ ] Step 2: fix what the screenshots show (overflow, contrast, spacing); each fix with its test where one can catch it.
- [ ] Step 3: commit "Screen: a demo backend to look at every page".

### Task 9: Check and ship

- [ ] `npx tsc --noEmit -p .`, `npx vitest run`, `npm run build`; CHANGELOG; PR; CI; merge.
