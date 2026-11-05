// App-wide context: navigation, toasts, settings, and the shared apply flow.

import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { api, errorMessage, type AppSettings } from "./api";

export type PageId =
  | "overview"
  | "tweak"
  | "gaming"
  | "diagnostics"
  | "network"
  | "startup"
  | "assistant"
  | "history"
  | "settings";

export interface Toast {
  id: number;
  tone: "good" | "warning" | "critical" | "info";
  title: string;
  detail?: string;
  action?: { label: string; run: () => void };
}

export interface ApplyIntent {
  tweakIds: string[];
  startupDisableIds?: string[];
  source: string;
  label: string;
  title?: string;
  description?: string;
  onDone?: (summary: ApplyOutcome) => void;
}

export interface ApplyOutcome {
  approved: boolean;
  txIds: string[];
  summary: string;
}

interface Ctx {
  page: PageId;
  navigate: (p: PageId, opts?: { prompt?: string }) => void;
  pendingPrompt: string | null;
  takePendingPrompt: () => string | null;
  toast: (t: Omit<Toast, "id">) => void;
  settings: AppSettings | null;
  setSettings: (s: AppSettings) => void;
  applyIntent: ApplyIntent | null;
  requestApply: (i: ApplyIntent) => void;
  closeApply: () => void;
  dataVersion: number;
  bumpData: () => void;
}

const AppCtx = createContext<Ctx | null>(null);

export function AppProvider({ children }: { children: ReactNode }) {
  const [page, setPage] = useState<PageId>("overview");
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [settings, setSettingsState] = useState<AppSettings | null>(null);
  const [applyIntent, setApplyIntent] = useState<ApplyIntent | null>(null);
  const [dataVersion, setDataVersion] = useState(0);
  const pending = useRef<string | null>(null);
  const seq = useRef(0);

  useEffect(() => {
    api.getSettings().then(setSettingsState).catch(() => {});
  }, []);
  useEffect(() => {
    document.body.classList.toggle("reduce-motion", !!settings?.reduce_motion);
  }, [settings?.reduce_motion]);

  const toast = useCallback((t: Omit<Toast, "id">) => {
    const id = ++seq.current;
    setToasts((ts) => [...ts.slice(-3), { ...t, id }]);
    setTimeout(() => setToasts((ts) => ts.filter((x) => x.id !== id)), t.action ? 9000 : 5500);
  }, []);

  const value: Ctx = {
    page,
    navigate: (p, opts) => {
      if (opts?.prompt) pending.current = opts.prompt;
      setPage(p);
    },
    pendingPrompt: pending.current,
    takePendingPrompt: () => {
      const p = pending.current;
      pending.current = null;
      return p;
    },
    toast,
    settings,
    setSettings: setSettingsState,
    applyIntent,
    requestApply: setApplyIntent,
    closeApply: () => setApplyIntent(null),
    dataVersion,
    bumpData: () => setDataVersion((v) => v + 1),
  };

  return (
    <AppCtx.Provider value={value}>
      {children}
      <div className="toasts" role="status" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.tone}`}>
            <ToneIcon tone={t.tone} />
            <div style={{ flex: 1, minWidth: 0 }}>
              <div className="t">{t.title}</div>
              {t.detail && <div className="d">{t.detail}</div>}
            </div>
            {t.action && (
              <button
                className="btn sm"
                onClick={() => {
                  t.action!.run();
                  setToasts((ts) => ts.filter((x) => x.id !== t.id));
                }}
              >
                {t.action.label}
              </button>
            )}
          </div>
        ))}
      </div>
    </AppCtx.Provider>
  );
}

export function useApp() {
  const c = useContext(AppCtx);
  if (!c) throw new Error("useApp outside provider");
  return c;
}

export function ToneIcon({ tone }: { tone: string }) {
  if (tone === "good")
    return (
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden>
        <circle cx="8" cy="8" r="6.3" />
        <path d="M5.2 8.2l1.9 1.9 3.8-4" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    );
  if (tone === "warning" || tone === "critical")
    return (
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden>
        <path d="M8 2.2l6.2 11H1.8z" strokeLinejoin="round" />
        <path d="M8 6.5v3.2M8 11.6v.1" strokeLinecap="round" />
      </svg>
    );
  return (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden>
      <circle cx="8" cy="8" r="6.3" />
      <path d="M8 7.3v3.6M8 5.2v.1" strokeLinecap="round" />
    </svg>
  );
}

/** Load data with loading/error state; re-runs when `deps` change. */
export function useLoad<T>(fn: () => Promise<T>, deps: unknown[] = []) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    let alive = true;
    setLoading(true);
    fn()
      .then((d) => {
        if (alive) {
          setData(d);
          setError(null);
        }
      })
      .catch((e) => alive && setError(errorMessage(e)))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, tick]);
  return { data, error, loading, reload: () => setTick((t) => t + 1), setData };
}
