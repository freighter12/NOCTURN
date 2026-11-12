# Nocturn Music for iPhone

A separate app in `mobile/` (branch `ios-music`): the desktop app's music side, for iPhone, sideloaded.

## What's in it

- **YouTube Music, straight from the phone** (`mobile/src-tauri/src/yt.rs`): search + suggestions, Home, albums, artists, playlists (long ones are paged), radio. Audio links come from the player API with the `VISIONOS` client — direct AAC (itag 140), no JavaScript decoding, no tokens — but it **needs a visitor ID** (from `youtube.com/sw.js_data`); without one most songs answer "sign in to confirm you're not a bot".
- **Native player** (`mobile/plugins/nocturn/ios/Sources/Player.swift`): AVQueuePlayer with the next song preloaded (gapless), background audio, lock screen / Control Center / AirPods controls, pause on headphone unplug, YouTube Music radio when the queue runs out. It resolves links itself (`YouTube.swift`) so songs keep changing while the web view sleeps.
- **Liquid Glass** (`Chrome.swift`): the iOS 26 `UITabBarController` (Home, Library, Search tab), minimizes on scroll, mini player as the `bottomAccessory`, full-screen Now Playing in SwiftUI with `.glass` buttons. The one web view is moved into whichever tab is showing.
- **Lyrics like the desktop** (`lyrics.rs` + `Lyrics.swift`): same LRCLIB lookup (word timings, slowed/sped-up versions stretched), the desktop's own timing model reused (`src/music/lyricsTiming.ts` via the `@shared` alias), drawn natively: word-by-word, instrumental dots, auto-scroll, tap to seek, Later/Earlier, Words/Lines, Bluetooth latency included.
- **Playlist import by link** (`import.rs`): Spotify, Deezer, YouTube, YouTube Music. Spotify/Deezer songs are matched on YouTube Music (50/50 on a 50-song test). Imported playlists live in Library › Your Playlists (`library.json` in the app's data folder).

## Building

Pushing to `mobile/**` or `src/music/**` runs `.github/workflows/ios-music.yml` on GitHub's macOS runners (default `macos-26-intel` — Apple Silicon runners were often out of capacity): `tauri ios init --ci`, `tauri ios build --no-sign`, then the unsigned `.ipa` + a SideStore `source.json` go to the rolling pre-release **nocturn-music-ios** (never "latest", so the desktop updater is unaffected).

- App file: https://github.com/freighter12/NOCTURN/releases/download/nocturn-music-ios/NocturnMusic.ipa
- SideStore source: https://github.com/freighter12/NOCTURN/releases/download/nocturn-music-ios/source.json

Test the screens on Windows: `cd mobile`, then `npx tauri dev` with a phone-sized window (see `native.ts` — a computer-only `<audio>` stand-in plays instead of the Swift player, out loud).

## Installing (free Apple ID)

iloader on the PC → **Import IPA** → `NocturnMusic.ipa` (phone plugged in, unlocked). Or SideStore › Sources › + the source link, once SideStore can sign in. Apps expire after 7 days; renew with iloader or SideStore.

## Debugging a crash on the phone

Phone plugged in, then `python -m pymobiledevice3 crash pull <folder> --match Nocturn` and read the triggered thread in the `.ips` (JSON from the second line). This found both launch crashes so far.

## Status (2026-11-11)

- Build 0.1.0.6 is the current one. Builds 3–5 crashed on launch (adding Tauri's view controller to a tab while it was still the window root; then UIKit's assert on reused tab screens) — both fixed; **0.1.0.6 is not yet confirmed on the phone.**
- SideStore on the user's phone can't sign in (Apple error −80045; fix = turn off On-Device Anisette, SideStore (New) server, Fetch + Save as Overrides, Reset adi.pb) and needs LocalDevVPN connected to use its pairing file.
- Not yet tested on a device: background playback, lock screen, tab bar minimizing, Now Playing, lyrics view, import on the phone.

## Added 2026-11-12 (afternoon)

- **Crossfade** (`Player.swift`): two `AVQueuePlayer`s; with crossfade on, the next song waits silently on the standby player and the two swap with an equal-power fade. Same-album songs stay gapless.
- **Listening log** (`Stats.swift` → Application Support/stats.json) and **Nocturn Wrapped** (`src/stats.ts`, `src/pages/Stats.tsx`).
- **PC sync** (`src-tauri/src/sync.rs`, `src/sync.ts`, `QRScanner.swift`; PC side `src-tauri/src/phone_sync.rs` in the desktop app). The QR code is JSON `{"t":"nocturn-sync","v":1,"h":ip,"p":47821,"k":key,"n":name}`; both sides accept only private addresses.
- **Lyrics looks** (`Lyrics.swift`, UserDefaults `lyrics.color/style/size/align/blur`) and long-press **Sync to This Line**.
- **Backgrounds**: 11 new + Seasons (`Themes.swift`).
- **Smoothness** (`theme.refresh`), **Play Alongside Other Apps** (`audio.mix`), **Carry On After Interruptions** (`audio.resume`), **update banner** (`src/update.ts`, Rust `check_update` reads source.json).
- New Swift commands must be added to `plugins/nocturn/build.rs` **and** `permissions/default.toml`, or the web side gets "not allowed".
