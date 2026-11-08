// Wind-down, page side. The backend runs the schedule, the warm screen and
// the bedtime action (so it works from the tray); this carries out the music
// parts — fade down, start a calm playlist, fade out at bedtime — and holds
// the status for the page, Home and the tray.

import { listen } from "@tauri-apps/api/event";
import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { errorMessage, winddown, type WindDownConfig, type WindDownStatus } from "../lib/api";
import { useApp } from "../lib/app";
import { useMusic } from "../music/MusicProvider";
import { resolveList } from "../music/SmartLists";

interface WindDownCtx {
  status: WindDownStatus | null;
  start: (minutes?: number) => Promise<void>;
  stop: () => Promise<void>;
  snooze: (minutes: number) => Promise<void>;
  cancelSleep: () => Promise<void>;
  save: (c: WindDownConfig) => Promise<void>;
}

const Ctx = createContext<WindDownCtx | null>(null);

const ease = (x: number) => x * x * (3 - 2 * x);

export function WindDownProvider({ children }: { children: ReactNode }) {
  const { toast } = useApp();
  const m = useMusic();
  const [status, setStatus] = useState<WindDownStatus | null>(null);
  const prevPhase = useRef<WindDownStatus["phase"] | null>(null);
  const mRef = useRef(m);
  mRef.current = m;

  useEffect(() => {
    winddown.status().then(setStatus).catch(() => {});
    const un = listen<WindDownStatus>("winddown", (e) => setStatus(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  // Music follows the phase.
  useEffect(() => {
    if (!status) return;
    const c = status.config;
    const m = mRef.current;
    const was = prevPhase.current;
    prevPhase.current = status.phase;
    if (status.phase === "off") {
      m.setDim(1);
      return;
    }
    // Just started: put on the chosen playlist if nothing's playing.
    if (was === "off" && status.phase === "winding" && c.playlist && !m.playing) {
      resolveList(c.playlist, m.tracks, m.playlists).then((ids) => {
        if (ids.length) mRef.current.play(ids);
      });
    }
    m.setDim(c.music_fade ? 1 - (1 - c.music_floor) * ease(status.progress) : 1);
    // Bedtime: let the music go.
    if (was === "winding" && status.phase === "night" && c.music_stop && m.playing) {
      m.fadeOutAndPause(30);
    }
  }, [status]);

  const run = useCallback(
    async (f: () => Promise<WindDownStatus>, ok?: string) => {
      try {
        setStatus(await f());
        if (ok) toast({ tone: "good", title: ok });
      } catch (e) {
        toast({ tone: "critical", title: "Wind-down didn't change", detail: errorMessage(e) });
      }
    },
    [toast],
  );

  const value: WindDownCtx = {
    status,
    start: (minutes) => run(() => winddown.start(minutes ?? status?.config.lead_min ?? 30)),
    stop: () => run(() => winddown.stop()),
    snooze: (minutes) => run(() => winddown.snooze(minutes), `Bedtime moved ${minutes} minutes later`),
    cancelSleep: () => run(() => winddown.cancelSleep(), "Your PC won't sleep"),
    save: (c) => run(() => winddown.save(c)),
  };

  // Tray: "Wind down now" / "Stop winding down".
  const v = useRef(value);
  v.current = value;
  useEffect(() => {
    const un = listen<string>("tray", (e) => {
      if (e.payload !== "winddown") return;
      if (v.current.status?.phase === "off") v.current.start();
      else v.current.stop();
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useWindDown() {
  const c = useContext(Ctx);
  if (!c) throw new Error("useWindDown outside WindDownProvider");
  return c;
}

/** "11:30 pm" in the user's own clock style. */
export const clock = (iso: string | null | undefined) => (iso ? new Date(iso).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) : "—");
