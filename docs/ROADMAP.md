# NOCTURN — advanced feature roadmap

Plan for the "Advanced Feature Expansion" brief, written after inspecting the
codebase on 2026-11-05. Status per module: **Exists** (working today),
**Partial** (some of the brief is covered), **Missing**.

## Current state vs the brief

| # | Module | Status | What exists today | Main gaps |
|---|--------|--------|-------------------|-----------|
| 1 | Process manager | Partial | Grouped live list (CPU/RAM/disk, 3 s), two-step End, protected-process list, AI can close apps after approval | Priority, affinity, search, background/app split, heavy-process flags, usage stats, per-app rules |
| 2 | Gaming optimiser | Partial | Readiness checks, Gaming profile, power plans, background process list | Game detection, per-game profiles, overlay detection, bottleneck monitor, session history, gaming modes |
| 3 | Hardware monitoring | Partial | CPU total/per-core/clock, RAM, GPU usage/VRAM (all vendors), GPU temp (NVIDIA via `nvidia-smi` every 5 s) | GPU clocks/power, disk I/O, memory pressure, history beyond 2 min, refresh rate, pause, thresholds |
| 4 | Startup intelligence | Partial | Run keys + Startup folders, enable/disable (journaled), file-missing flag | Publisher/description, signature check, suspicious-entry rules, per-entry AI explanation, NOCTURN change list |
| 5 | Service intelligence | Partial | Read-only list with notes for common services | Descriptions, dependencies, tiering, approved start-type changes with rollback |
| 6 | Network | Partial | Adapters, DNS, Wi-Fi, ping/jitter/loss, DNS timing, flush DNS | Traceroute, DNS provider switching, speed test, history graph |
| 7 | Driver centre | Partial | Driver list, age and signature flags | Problem devices, official support links, update history |
| 8 | Storage intelligence | Partial | Drive usage, temp-file cleanup (one-time action) | Folder usage map, large files, duplicates, Recycle Bin, review-before-delete |
| 9 | Customisation | Partial | ~24 catalogue settings with preview/rollback | Taskbar/Explorer/notification categories, "reset to Windows default" |
| 10 | Automatic maintenance | Missing | — | Scheduler, game-aware deferral, maintenance history |
| 11 | Crash analyser | Partial | Grouped event-log errors | Crash/BSOD/unexpected-shutdown classification, reliability history, dump summary |
| 12 | Benchmarking | Missing | — | CPU/memory/storage micro-benchmarks, boot time, FPS via PresentMon, before/after comparison |
| 13 | AI expansion | Partial | 12 read-only tools, approval-gated plans (tweaks, startup, close apps), per-item approval | Tools for every new module; plans that include new action types |
| 14 | Restore centre | Partial | Journal, per-transaction rollback, audit log | Batch rollback, approver column, recovery guide |
| 15 | Application profiles | Partial | Balanced / Gaming / Maximum / Custom tweak profiles | Cross-module profiles, automatic switching |
| 16 | Efficiency | Partial | 1 s sampler, 3 s process scan | `nvidia-smi` process spawned every 5 s; no pause; no configurable rate |

## Honest technical limits (decided up front)

- **CPU temperature / CPU power.** Windows exposes no vendor-neutral API. Tools that show it load a kernel driver (WinRing0 and similar), several of which are on Microsoft's vulnerable-driver blocklist. NOCTURN will not ship a kernel driver; CPU temperature stays "not available" rather than estimated.
- **GPU detail.** NVIDIA: temperature, clocks, power, fan through NVML (`nvml.dll`, installed with the driver) — loaded at runtime, no process spawning. AMD/Intel: usage and VRAM only (what Windows exposes) until a vendor SDK is integrated.
- **FPS, 1 % lows, frame times.** Need ETW present events. Planned via Intel's open-source PresentMon (Priority 3); until then NOCTURN shows CPU/GPU bottleneck data but no FPS.
- **CPU Sets.** Soft scheduling hints that applications may override. Affinity covers the user-facing need; CPU Sets are not implemented.
- **Startup impact.** Task Manager's figure has no public API. NOCTURN will show signature, publisher and file facts instead of an invented impact rating.

## Phases

### Priority 1 (done 2026-11-06 — four gaming modes shipped: Competitive, Maximum Quality, Balanced Gaming, Streaming)
1. **Process manager** — new *Processes* page: search, app/background split, priority and affinity changes (never Realtime, never protected processes), resource-heavy flags, session usage statistics, saved per-app rules applied automatically when the app starts (audited).
2. **Hardware monitor** — new *Hardware* page: NVML GPU telemetry replacing `nvidia-smi`, disk read/write and memory-pressure counters, per-minute history stored for 7 days, refresh rate and pause, warning thresholds with alerts.
3. **Startup intelligence** — publisher and description from version info, Authenticode check, rule-based "review this" flags with reasons, per-entry "Ask AI", NOCTURN change list.
4. **Gaming optimiser** — Steam/Epic/GOG game detection, running-game detection, per-game profiles (priority, affinity, GPU preference, fullscreen optimisations — journaled and reversible), overlay detection, bottleneck monitor and session history, five gaming modes previewed through the existing approval flow.

### Priority 2 (done and verified 2026-11-06)
Network (traceroute, DNS providers, history), storage (folder map, large files, duplicates, review-then-delete to Recycle Bin), customisation categories, service management with tiering and journaled start-type changes.

### Priority 3 (done 2026-11-06 — crash analyser, driver centre, benchmarks, scheduled maintenance; PresentMon FPS capture deferred)
Benchmarking (CPU/memory/storage, boot time, PresentMon FPS), crash analyser, driver centre, scheduled maintenance.

### Priority 4
AI tools for every module, cross-module application profiles with automatic switching, release hardening.
