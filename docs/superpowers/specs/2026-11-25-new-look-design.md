# Kite Pack's new look and layout

Approved by Nankhor from the mockups on the "Kite Pack new look" canvas (page "Every page"),
2026-11-25: "yeah nice it should have dark mode as well and yeah go on build it".

## Why

Today's screens are "too grey, too boring, not a polished app". The colourful directions were
"too much colour". What's wanted is a Stripe-like dashboard, shadcn-style: neutral surfaces,
crisp type, one ink colour. The games bring the colour. The layout must adapt to the window size,
and the game page stays.

## Goals

- **One design language**, light and dark, following the computer's setting unless Settings picks
  one. Neutral surfaces, hairline borders, 12 px cards, 8 px controls, black (light) or white
  (dark) primary buttons, Geist and Geist Mono bundled with the app (no network).
- **A sidebar for the things you do and the things on the console:**
  - Install (stream a .pkg to the PS5), Convert (a dump to a .fpkg), Pack (compress and send),
    History.
  - Under **PS5** with its status: My games, Files, Backups (game saves).
  - Settings at the bottom.
- **Every page adapts to the window:** below about 900 px the sidebar sits on top and the side
  column drops under the main one. The window itself never scrolls sideways.
- **The game page stays:** art, icon, title, story, details and screenshots from IGDB, as a card.

## Non-goals

- No new behaviour in part 1 beyond moving installs to their own page and the theme choice.
- No colour accents beyond status (green ready, amber waiting/warning, red failed) and the
  games' own art.

## Anti-goals

- Colour washes, glows and gradients as decoration.
- A page that only works full-screen.
- Losing what today's screens say: every message, button and number that exists now still exists.

## Approaches considered

1. **Tide glass:** the game's art floods the window. Rejected: too much colour.
2. **Sticker pop:** bold and playful. Rejected: too much colour.
3. **Dusk kite:** a sky scene with a kite. Rejected: too much colour.
4. **Dashboard (chosen):** Stripe/shadcn-like, neutral, light and dark.

## The parts, each its own pull request

1. **The new look and layout.** Design tokens and theme, sidebar shell, and drops sent to the
   right page. Install becomes its own page, taking packages out of Pack. Pack, History,
   My games, a game's page and Settings move to the new design, and Settings gets Appearance
   (System / Light / Dark).
2. **Convert.** A dump becomes a fake package (.fpkg) through Forge's package writer
   (`Format::Pkg`), compression off by default ("Off installs on every firmware"), saved to a
   Packages folder (Settings). Then optionally Install on PS5 (streamed), and optionally delete the
   .fpkg once the PS5 confirms it. Needs kstuff with fake-package support on the PS5.
3. **Files.** Browse the PS5's folders (internal /data, games /user/app, USB /mnt/ext*) and
   download a file to this computer. Read-only. Needs the helper to read those roots.
4. **Backups (game saves).** For one PS5 user, list each game's saves with when they changed and
   the last backup. Back up one game or all to this computer (`Save backups` folder, last 10
   copies per game) and restore a copy. Needs a Kite helper update to read and write the saves
   folder. As far as is known, saves restore on the same console and user only.

The install queue ("Up next") drawn on the Install mockup is not in these parts; it comes later
if wanted.

## Part 1 in detail

- **Theme:** tokens as CSS variables on `:root` (light) and `.dark` (dark). `<html>` gets `dark`
  when the choice is Dark, or System while the computer is dark, and follows it live. The choice is
  stored like the update preferences.
- **Sidebar items for parts 2–4 appear only once their part ships.**
- **Install page:**
  - A package (.pkg/.fpkg) opens here, wherever it is dropped.
  - Left: the package as a game card (art, icon, title, kind, version, file size, title ID).
  - Right while ready: "Install on PS5" and the ready note. While running: a progress card
    ("Installing on PS5", bar, "X of Y on the PS5", speed, time left, Stop).
  - Done, failed and waiting messages as today. Packing a PS5 game package stays possible from
    the Pack page.
- **Pack page:**
  - A dump (folder or archive) opens here.
  - Left: the game card.
  - Right: Output (Save as, Level, Packs into), PS5 (Send to PS5, Delete the packed folder),
    DLC, then the main button.
  - While running: the one job bar, in a card, with Stop.
- **Running job:** only one job runs at a time, as now. Its page's sidebar item shows its progress
  badge, and the page shows its progress.
- **History:** a table (game, what, size, when, result), filterable by All, Install, Pack, with
  Clear history and Show folder where a packed copy stayed.
- **My games:**
  - Free space per drive at the top (from the helper's free-space call).
  - The games as a grid of cards with search, opening a game's page: the game card plus
    "Space on the PS5" (game, update, DLC) and its DLC, Measure again.
  - With no PS5 set up: a card pointing to Settings.
- **Settings:** cards for PS5, This computer, Game details, Appearance and Updates, keeping every
  setting there is today.
- **Accessibility:**
  - Real links and buttons, keyboard focus visible everywhere.
  - Text contrast at least 4.5:1 in both themes.
  - Motion only where it shows a change.

## Left to the builder

Class names, how the stylesheet is split, and how App's views are factored. Keep every visible
message the tests rely on unless a test is updated for a real change.
