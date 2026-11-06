# NOCTURN development checklist

Status key: ✅ done and verified · 🟡 done, not fully verified · ⬜ not started

Last updated: 2026-11-06

## Phase 1–2 — Foundations

- ✅ Tauri 2 + React 19 + TypeScript + Rust, modular backend (`system/`, `tweaks/`, `ai/`, `security/`, `settings`, `logging`, `db`)
- ✅ Design system (`src/styles/global.css`): dark lunar palette, validated chart series colours
- ✅ Sidebar navigation with all nine sections; custom title bar
- ✅ Typed IPC bridge (`src/lib/api.ts`) mirroring every Rust command
- ✅ SQLite persistence (settings, change journal, conversations, audit log)
- ✅ Rolling file logs + audit log; graceful startup/shutdown verified on Windows 11

## Phase 3 — NOCTURN Tweak

- ✅ Hardware/OS inventory: CPU, GPU, RAM modules, board/BIOS, disks, OS build (real WMI/registry data)
- ✅ Live telemetry (1 s): CPU total/per-core, RAM, GPU usage/VRAM, network throughput, disks, uptime
- 🟡 Temperatures: GPU via `nvidia-smi` on NVIDIA only; CPU temperature is shown as "not exposed" (Windows has no vendor-neutral API)
- ✅ Process list with grouped view and two-step "End" (core Windows processes refused by the backend)
- ✅ Tweak catalogue (24 documented settings: gaming, performance, network, privacy) with live status
- ✅ Profiles: Balanced, Gaming, Maximum Performance, Custom — previewed before applying
- ✅ Write-ahead journal, settings backup file, per-change verification, restore-point offer for admin changes
- ✅ Rollback from History (verified live: apply → registry 0 → rollback → registry restored to "not set")
- ✅ Startup Manager: Run keys + Startup folders, enable/disable via StartupApproved (non-destructive), undo
- ✅ Power plans: list and switch (journalled, undoable)
- ✅ Diagnostics page: hardware, event log (grouped), drivers, services (read-only by design), restore points
- ✅ Network page: adapters/DNS/gateway, Wi-Fi details, stability test (latency/jitter/loss + DNS timing)
- ✅ Health check with evidence-backed findings on the dashboard

## Phase 4–5 — NOCTURN AI

- ✅ Provider-independent backend: Anthropic (native streaming) and OpenAI-compatible endpoints
- ✅ API keys in Windows Credential Manager only; never readable by the UI
- ✅ 12 read-only system tools + `propose_plan` (catalogue IDs only; validated server-side)
- ✅ Assistant UI: conversation list, streaming text/thinking, tool trace, markdown + code blocks, usage display
- ✅ Plan cards → shared ApplyFlow preview → approval → result sent back to the AI for verification
- ✅ Missing-key / error handling surfaces a clear message and restores the user's text
- 🟡 Live conversation against a real provider not exercised in this environment (no API key configured)
- ✅ Settings: provider, model, effort, max tokens, system-access toggle, custom instructions, connection test

## Advanced expansion — Priority 1 (see ROADMAP.md)

- ✅ Process manager page: search, app/background split, priority + affinity, heavy flags, per-app launch rules
- ✅ Hardware page: NVML GPU telemetry, disk I/O, memory pressure, 7-day history, refresh rate/pause, alerts
- ✅ Startup intelligence: publisher, description, Authenticode, "review this" reasons
- ✅ Gaming optimiser backend: Steam/Epic/GOG + custom game detection, running-game detection, session recording with CPU/GPU-limited estimate, overlay detection, per-game profiles (GPU preference, fullscreen optimisations — journaled; priority/CPUs — launch rule)
- ✅ Gaming page: live session + bottleneck panel, overlays, gaming modes via ApplyFlow, game library with add/remove, per-game profile editor (preview → apply → Undo), session history (verified in the real app with 10 Steam games; preview only — apply covered by the HKCU round-trip test)
- ✅ Removing priority/CPUs from a game profile deletes the launch rule it created (never a user-made rule)
- ⬜ Way to mark a detected entry as "not a game" (e.g. Steam utilities like Borderless Gaming currently start sessions)
- ⬜ Executable ranking picks the launcher for some games (Assetto Corsa: `AssettoCorsa.exe` ranked above `acs.exe`); the user can choose in the profile editor

## Advanced expansion — Priority 2

- ✅ Network — traceroute: native ICMP with per-hop TTL (no `tracert` process), streamed hop by hop, private-address tagging, stops after 5 silent hops (verified live: 9 hops to 1.1.1.1)
- ✅ Network — stability test history: every test stored (1-year retention), Router vs Internet latency chart (verified live)
- ✅ Network — DNS provider switching (Automatic / Cloudflare / Google / Quad9) per adapter, IPv4 + IPv6, via native `SetInterfaceDnsSettings`; new journaled `DnsServers` op with rollback; elevated worker accepts only the fixed provider lists (rollback: plain IPs of the right family). Verified live with UAC on Wi-Fi: Automatic → Cloudflare (IPv4 + IPv6 read back) → Undo → Automatic, no static NameServer left, resolution working
- ✅ Storage page: scan a drive or folder (junctions/symlinks never followed, OneDrive online-only files skipped so nothing downloads), folder usage map with drill-down, 200 largest files, duplicates (size → first 64 KiB → SHA-256), Recycle Bin size and empty (two-step confirm). Verified live: 680k files / 138 GB scanned in 38 s
- ✅ Review-then-recycle: only files from the last scan, unchanged since, outside Windows/Program Files/ProgramData/system files; Windows warns before any permanent delete; at least one copy of every duplicate set is always kept; audit-logged. Verified live end to end on throwaway files
- ✅ Customise page: 19 per-user settings in Taskbar (alignment, search, Task View, combine buttons, clock seconds, End task), File Explorer (extensions, hidden files, open-to, compact view, full path, recent/frequent in Home, sync-provider banners) and Notifications (banners, lock screen, sounds, welcome experience, "finish setting up"); choices are compiled in, HKCU only (elevated worker refuses them), preview → apply → Undo/History. "Windows default" removes the value via a new journaled `RegistryDelete` op instead of guessing a default; per-section and all-settings reset. Settings change broadcast after apply/rollback. Verified live: apply → registry 1 → Undo → 0; section-reset preview against the real values (not applied); real-registry round-trip test (apply, reset, roll back both → byte-identical original)
- ✅ "Restart File Explorer" button (for compact view, Home lists, sync banners), two-step confirm — verified by the user on their PC
- ✅ Services page: native SCM enumeration (287 services in ~170 ms) with descriptions, program, publisher, account, dependencies and dependents; four tiers — Protected (security, updates, core; never changed, not even by rollback), Windows (read-only), Optional (31 reviewed Windows features, each saying what stops working; the program must be Microsoft's), Third-party (apps/drivers; anti-cheat, updater and driver-helper hints). Verified live on the real list, preview and refusals included
- ✅ Service start-type changes: journaled `ServiceStartType` op capturing start type + delayed flag, elevated worker re-reads the service and re-applies tier and dependency rules (refuses Disabled while an automatic service depends on it); only start types change, never stop/delete. Verified live with UAC: Retail Demo Service Manual → Disabled (elevated worker captured the original) → rolled back from History → Manual; refusal path tested on real protected services

## Advanced expansion — Priority 3

- ✅ Crash analyser (Diagnostics › Crashes): 90 days of blue screens (WER 1001 + Kernel-Power 41), unexpected shutdowns (forced power-off and sleep detected), EventLog 6008, WHEA hardware errors, app crashes (Application Error 1000, filtered by provider) and hangs; one incident's several records merged; 28 stop codes and 13 exception codes explained; faulting-module attribution (graphics driver, overlay, app's own code, generic Windows library); rule-based findings with steps; per-day timeline (validated palette, hover, table view) and Reliability Monitor stability index; crash-dump inventory; Ask AI per finding/app. Verified live on this PC: 1 unexpected shutdown (2 records merged), 8 app crashes in 4 apps, stability 8.2
- ⬜ Driver centre
- ⬜ Benchmarks
- ⬜ Scheduled maintenance

## Phase 6 — Testing and release

- ✅ Rust unit/integration tests: 122 passing (3 ignored tests with real side effects, run manually: registry round-trip, Customise round-trip, Recycle Bin)
- ✅ Frontend unit tests (vitest): chat transcript folding, formatters
- ✅ Typecheck + production frontend build
- ✅ All pages loaded in the real app with zero console errors (CDP walkthrough)
- ⬜ Signed installer (`npm run tauri build` → NSIS/MSI); needs a code-signing certificate
- ⬜ Windows 10 verification (only Windows 11 tested so far)
- ⬜ End-to-end AI test with a real key: plan proposal → approval → verification

## Deliberate limitations

- Only start types of optional and third-party services can change (with approval); security, update and core services are protected. Defender, Update, firewall, UAC and SmartScreen are never touched.
- Network reset, driver updates and BIOS settings (XMP/EXPO) are explained, not executed.
- The assistant can't run commands; it can only propose catalogue changes the user approves.
