# NOCTURN — Ecosystem Expansion plan

Written 2026-11-06 from the "Ecosystem Expansion Specification", after
auditing the codebase. Status per phase is tracked in `CHECKLIST.md`.

## Rule zero: the existing product is the foundation

Every page that exists today keeps its page ID, its backend commands and its
behaviour. The expansion adds modules and re-groups navigation; it does not
rewrite the optimiser, AI, diagnostics or monitoring.

Baseline at the start (2026-11-06): 144 backend + 2 kernel + 9 frontend tests
passing; release build walked through all 17 pages and 6 Diagnostics tabs with
zero console errors.

### Where today's pages live in the new structure

| New section | Existing pages (unchanged IDs) |
|---|---|
| Home | *new* widget dashboard (the current Overview stays as System › Overview) |
| Library | *new* |
| Music | *new* |
| Workspaces | *new* |
| AI | `assistant` |
| Gaming | `gaming`, `benchmarks` |
| System | `overview`, `hardware`, `processes`, `tweak`, `customise` (renamed **Windows settings**), `profiles`, `diagnostics`, `network`, `storage`, `startup`, `services`, `maintenance`, `history` |
| Customise | *new* **Experience Studio** |
| Settings | `settings` (+ privacy & integrations) |

## Architecture

Frontend modules (React, `src/`), each with its own folder and a small public
interface; they talk through the shell, not to each other directly.

| Module | Owns | Talks to |
|---|---|---|
| **Shell** (`src/shell/`) | navigation model, routing, theming, preferences, toasts, the persistent player bar | every module via the nav registry |
| **System** | the existing pages | existing commands (unchanged) |
| **Library** (`src/library/`) | apps, categories, launching, integrations | backend `library` commands |
| **Workspaces** (`src/workspaces/`) | workspaces, layouts, app groups | Library (app IDs), Music (playlist IDs), Shell (wallpaper) |
| **Music** (`src/music/`) | catalogue sources, playback engine, queue, library, playlists, player modes | backend `music` commands; audio element owned by the shell |
| **Studio** (`src/studio/`) | themes, layout, widgets, accessibility | preferences store |
| **AI** | existing assistant + new tools | backend tool registry |

Backend (Rust, `src-tauri/src/`): new modules `prefs`, `library`, `workspaces`,
`music`, each with its own tables or settings keys, its own commands, and
tests. Anything that launches programs or reads files goes through validated
commands, like the rest of NOCTURN.

### Preferences

One preference document, versioned, stored in SQLite (`ui.prefs`), with
categories: appearance, layout, navigation, music, workspaces, accessibility,
behaviour. The frontend reads it once, applies it live (CSS variables, nav
model, player mode) and writes changes back debounced. Unknown or invalid
values fall back to defaults field by field, so a bad value can't make the UI
unusable. Studio offers "reset this section" and "reset everything".

### Guard rails for customisation

- Text/background contrast is checked when a theme is saved; colours that
  would make text unreadable are refused with an explanation.
- System and Settings can't be hidden from navigation, so the user can always
  get back.
- Layout changes are reversible ("reset layout").

## Honest limits (decided up front)

- **Nocturn Music catalogue.** A free, ad-free streaming catalogue of
  commercial music needs licensing deals and servers, which code alone can't
  provide. Phase 6 builds the complete player, library, playlists, queue and
  player modes on a *catalogue-source* interface. Sources at launch: the
  user's own music files (local import, never uploaded). The interface is
  ready for a NOCTURN-hosted licensed catalogue (direct licences, artist
  uploads, public-domain/CC collections) without changes to the app. No
  unauthorised downloading, ripping or stream capture of any kind.
- **Third-party apps.** Most communication and entertainment services
  (Discord, Spotify, WhatsApp, Steam…) don't permit being embedded or
  automated. They appear in the Library and open through their official app or
  official links (deep links) — never by embedding their web app against their
  terms or scraping. Embedded web views are only used for sites the user adds
  themselves.
- **Workspaces** organise and launch apps; they don't move or resize other
  apps' windows (Windows already does that with Snap layouts and virtual
  desktops).

## Privacy

- No analytics, trackers or ads — none exist today and none are added.
- Everything new is stored locally in NOCTURN's database.
- Settings › Privacy lists exactly what is stored, what leaves the PC (only:
  AI requests to the provider the user configured; network tests' pings) and
  which integrations exist.

## Phases (estimates)

| Phase | Scope | Est. |
|---|---|---|
| 1 | Audit, this plan, baseline | 0.5 h |
| 2 | Shell: grouped data-driven navigation, Home dashboard, preference system | 3 h |
| 3 | Experience Studio: themes, accent, layout, nav customisation, widgets, accessibility | 4 h |
| 4 | App Library: installed-app detection, manual add, categories, favourites, search, launch, deep links | 3 h |
| 5 | Workspaces: create/rename/icon/wallpaper, app collections, layouts, quick switching, launch group | 3 h |
| 6 | Music foundation: local catalogue, playback engine, queue, library, playlists, four player modes, persistent player | 5 h |
| 7 | AI: tools for library, workspaces, music; approval for anything that launches or changes things | 2 h |
| 8 | Polish and testing across everything | 2–3 h |
