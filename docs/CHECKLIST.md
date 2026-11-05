# NOCTURN development checklist

Status key: ✅ done and verified · 🟡 done, not fully verified · ⬜ not started

Last updated: 2026-11-05

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

## Phase 6 — Testing and release

- ✅ Rust unit/integration tests: 49 passing (1 ignored destructive round-trip test, run manually)
- ✅ Frontend unit tests (vitest): chat transcript folding, formatters
- ✅ Typecheck + production frontend build
- ✅ All nine pages loaded in the real app with zero console errors (CDP walkthrough)
- ⬜ Signed installer (`npm run tauri build` → NSIS/MSI); needs a code-signing certificate
- ⬜ Windows 10 verification (only Windows 11 tested so far)
- ⬜ End-to-end AI test with a real key: plan proposal → approval → verification

## Deliberate limitations

- Windows services are never modified; Defender, Update, firewall, UAC and SmartScreen are never touched.
- Network reset, driver updates and BIOS settings (XMP/EXPO) are explained, not executed.
- The assistant can't run commands; it can only propose catalogue changes the user approves.
