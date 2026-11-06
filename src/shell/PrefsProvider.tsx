// Loads the preference document once, applies it live (no restart needed)
// and saves changes back, debounced.

import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { api } from "../lib/api";
import { PAGE_IDS, SECTION_IDS } from "./nav";
import { DEFAULT_PREFS, sanitize, type Prefs } from "./prefs";
import { applyAppearance } from "./theme";

interface PrefsCtx {
  prefs: Prefs;
  /** True once the stored preferences have been read. */
  loaded: boolean;
  /** Change preferences; the updater receives the current document. */
  update: (fn: (p: Prefs) => Prefs) => void;
  /** Reset one category (or everything) to defaults. */
  reset: (category?: keyof Omit<Prefs, "version">) => void;
}

const Ctx = createContext<PrefsCtx | null>(null);

export function PrefsProvider({ children }: { children: ReactNode }) {
  const [prefs, setPrefs] = useState<Prefs>(DEFAULT_PREFS);
  const [loaded, setLoaded] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    api
      .getPrefs()
      .then((raw) => setPrefs(sanitize(raw, PAGE_IDS, SECTION_IDS)))
      .catch(() => {})
      .finally(() => setLoaded(true));
  }, []);

  // Apply appearance and accessibility whenever they change.
  useEffect(() => applyAppearance(prefs), [prefs.appearance, prefs.accessibility, prefs.workspaces.active, prefs.workspaces.list]);

  const persist = useCallback((p: Prefs) => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => api.savePrefs(p).catch(() => {}), 400);
  }, []);

  const update = useCallback(
    (fn: (p: Prefs) => Prefs) =>
      setPrefs((cur) => {
        const next = sanitize(fn(cur), PAGE_IDS, SECTION_IDS);
        persist(next);
        return next;
      }),
    [persist],
  );

  const reset = useCallback(
    (category?: keyof Omit<Prefs, "version">) =>
      update((cur) => (category ? { ...cur, [category]: DEFAULT_PREFS[category] } : DEFAULT_PREFS)),
    [update],
  );

  return <Ctx.Provider value={{ prefs, loaded, update, reset }}>{children}</Ctx.Provider>;
}

export function usePrefs() {
  const c = useContext(Ctx);
  if (!c) throw new Error("usePrefs outside PrefsProvider");
  return c;
}
