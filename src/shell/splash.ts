// Bridge to the startup splash in index.html / public/splash.js.

import { useEffect } from "react";
import { useApp } from "../lib/app";
import type { IntroMode } from "./prefs";
import { usePrefs } from "./PrefsProvider";

interface SplashApi {
  ready(): void;
  remember(mode: IntroMode, reduceMotion: boolean): void;
  preview(mode: IntroMode, reduceMotion: boolean): void;
}

const splash = (): SplashApi | undefined => (window as unknown as { nocturnSplash?: SplashApi }).nocturnSplash;

/** Replay the intro in a given mode (Experience Studio preview). */
export function previewIntro(mode: IntroMode, reduceMotion: boolean) {
  splash()?.preview(mode, reduceMotion);
}

/**
 * Lets the splash go once preferences are loaded and the shell has painted,
 * and mirrors the intro mode and reduce-motion into localStorage so the next
 * launch picks them before first paint.
 */
export function SplashBridge() {
  const { prefs, loaded } = usePrefs();
  const { settings } = useApp();
  const reduce = prefs.accessibility.reduceMotion || !!settings?.reduce_motion;

  useEffect(() => {
    if (!loaded) return;
    // Two frames: the first commits the shell, the second has it on screen.
    let b = 0;
    const a = requestAnimationFrame(() => (b = requestAnimationFrame(() => splash()?.ready())));
    return () => {
      cancelAnimationFrame(a);
      cancelAnimationFrame(b);
    };
  }, [loaded]);

  useEffect(() => {
    if (loaded && settings) splash()?.remember(prefs.appearance.intro, reduce);
  }, [loaded, settings, prefs.appearance.intro, reduce]);

  return null;
}
