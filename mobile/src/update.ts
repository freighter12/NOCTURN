// "A new build is out": checked once a launch against the same file SideStore
// reads (GitHub release nocturn-music-ios), shown as a banner at the top and
// in Settings › About. A sideloaded app can't install itself, so the banner
// leads to the steps instead.
import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isNative, settings } from "./native";

export type Latest = { version: string; build: string; date: string; notes: string };
export type UpdateState = {
  checking: boolean;
  current: string;
  latest: Latest | null;
  available: boolean;
  error: string | null;
  /** banner hidden for this build */
  dismissed: boolean;
};

const DISMISS_KEY = "nocturn.update.dismissed";
let state: UpdateState = { checking: false, current: "", latest: null, available: false, error: null, dismissed: false };
const subs = new Set<() => void>();
const set = (p: Partial<UpdateState>) => {
  state = { ...state, ...p };
  subs.forEach((f) => f());
};

/** "0.1.0.24" vs "0.1.0.25" (numbers compared part by part). */
export function newer(a: string, b: string): boolean {
  const pa = a.split(".").map((x) => parseInt(x, 10) || 0);
  const pb = b.split(".").map((x) => parseInt(x, 10) || 0);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d) return d > 0;
  }
  return false;
}

/** The short build number people see ("Build 25"). */
export const buildLabel = (b: string) => `Build ${b.split(".").pop() || b}`;

function dismissedFor(): string | null {
  try {
    return localStorage.getItem(DISMISS_KEY);
  } catch {
    return null;
  }
}

export const updates = {
  async check() {
    if (state.checking) return;
    set({ checking: true, error: null });
    try {
      const [s, latest] = await Promise.all([settings.get(), invoke<Latest>("check_update")]);
      const current = s.build;
      const available = !!current && !!latest.build && newer(latest.build, current);
      set({ checking: false, current, latest, available, dismissed: available && dismissedFor() === latest.build });
    } catch (e) {
      set({ checking: false, error: String(e) });
    }
  },
  dismiss() {
    if (!state.latest) return;
    try {
      localStorage.setItem(DISMISS_KEY, state.latest.build);
    } catch {
      /* storage blocked */
    }
    set({ dismissed: true });
  },
  /** Once per launch, a few seconds in (after the start-up animation). */
  startupCheck() {
    if (isNative) setTimeout(() => updates.check(), 4000);
  },
};

export function useUpdates(): UpdateState {
  return useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => state,
  );
}
