// Live telemetry store. The backend emits one `telemetry` event per second;
// this module keeps a rolling history and exposes it via a React hook.

import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";
import { api, type Sample } from "./api";

const MAX = 120;
let history: Sample[] = [];
let snapshot: { latest: Sample | null; history: Sample[] } = { latest: null, history };
const subs = new Set<() => void>();
let started = false;

function publish() {
  snapshot = { latest: history[history.length - 1] ?? null, history };
  subs.forEach((f) => f());
}

export async function startTelemetry() {
  if (started) return;
  started = true;
  try {
    const initial = await api.telemetryHistory();
    if (initial.length) {
      history = initial.slice(-MAX);
      publish();
    }
  } catch {
    /* backend not ready yet; live events will fill in */
  }
  await listen<Sample>("telemetry", (e) => {
    history = [...history.slice(-(MAX - 1)), e.payload];
    publish();
  });
}

function subscribe(f: () => void) {
  subs.add(f);
  return () => subs.delete(f);
}

export function useTelemetry() {
  return useSyncExternalStore(subscribe, () => snapshot);
}

export function primaryGpu(s: Sample | null) {
  if (!s || !s.gpus.length) return null;
  // Prefer the GPU doing the most work / with dedicated VRAM.
  return [...s.gpus].sort((a, b) => (b.vram_total ?? 0) - (a.vram_total ?? 0))[0];
}
