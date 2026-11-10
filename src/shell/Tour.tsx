// First-run guided tour: a spotlight walks through the sidebar and the
// search box, opening each page as it goes. Runs once (prefs.tour.done);
// replay from Settings or Ctrl+K → "tour".

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useApp, type PageId } from "../lib/app";
import { usePrefs } from "./PrefsProvider";

interface Step {
  /** data-tour value of the element to spotlight; none = centred card. */
  target?: string;
  /** Page to open while this step shows. */
  page?: PageId;
  title: string;
  body: string;
}

export const TOUR_STEPS: Step[] = [
  { title: "Welcome to NOCTURN", body: "Your PC, music, games and AI in one calm place. This quick tour shows you around in about a minute." },
  { target: "home", page: "home", title: "Home", body: "Your dashboard. Widgets show your PC's health, music and shortcuts. Choose Arrange to drag them where you want, or change your greeting in Customise." },
  { target: "library", page: "library", title: "Library", body: "Every game and app on this PC, from every launcher, in one place. Open anything from here." },
  { target: "music", page: "music", title: "Music", body: "Nocturn Music plays your own songs, with synced lyrics, smart playlists and AI shuffle. No ads, ever." },
  { target: "workspaces", page: "workspaces", title: "Workspaces", body: "Set up spaces for gaming, work or study. Each brings its own look, apps and dashboard." },
  { target: "winddown", page: "winddown", title: "Wind down", body: "A calmer end to the day: the screen warms, music fades and notifications go quiet." },
  { target: "ai", page: "assistant", title: "Nocturn AI", body: "Ask about your PC in plain words: why it's slow, what a setting does, what to try next. You can set up free local AI in one click." },
  { target: "gaming", page: "gaming", title: "Gaming", body: "See what limits your games, set per-game options, and compare before and after with benchmarks." },
  { target: "system", page: "overview", title: "System", body: "The optimiser and system tools: processes, startup apps, storage, network and more. Every change is recorded first, so it can be rolled back." },
  { target: "search", title: "Search everything", body: "Press Ctrl+K from anywhere to jump to a page, open an app, play music or run an action." },
  { target: "studio", page: "studio", title: "Make it yours", body: "Themes (some of them animated), page backgrounds, your greeting, the sidebar and the player are all in Customise." },
  { page: "home", title: "You're all set", body: "Replay this tour any time from Settings, or search for \"tour\" with Ctrl+K." },
];

const START_EVENT = "nocturn:tour";
/** Start the tour from anywhere (Settings, command palette). */
export function startTour() {
  window.dispatchEvent(new Event(START_EVENT));
}

interface Box { x: number; y: number; w: number; h: number }

function findTarget(id?: string): HTMLElement | null {
  if (!id) return null;
  const el = document.querySelector<HTMLElement>(`[data-tour="${id}"]`);
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0 ? el : null;
}

export function Tour() {
  const { prefs, loaded, update } = usePrefs();
  const { navigate } = useApp();
  const [step, setStep] = useState<number | null>(null);
  const [box, setBox] = useState<Box | null>(null);
  const card = useRef<HTMLDivElement>(null);
  const [cardPos, setCardPos] = useState<{ left: number; top: number } | null>(null);
  const reduce = prefs.accessibility.reduceMotion;

  // Auto-start once: after the splash has gone and while the window is visible
  // (not when started hidden in the tray).
  useEffect(() => {
    if (!loaded || prefs.tour.done) return;
    const t = window.setInterval(() => {
      if (document.getElementById("splash") || document.visibilityState !== "visible") return;
      window.clearInterval(t);
      setStep(0);
    }, 400);
    return () => window.clearInterval(t);
  }, [loaded, prefs.tour.done]);

  useEffect(() => {
    const on = () => setStep(0);
    window.addEventListener(START_EVENT, on);
    return () => window.removeEventListener(START_EVENT, on);
  }, []);

  const finish = useCallback(() => {
    setStep(null);
    setBox(null);
    if (!prefs.tour.done) update((p) => ({ ...p, tour: { done: true } }));
  }, [prefs.tour.done, update]);

  const current = step === null ? null : TOUR_STEPS[step];

  // Open the step's page, then measure its target.
  useEffect(() => {
    if (!current) return;
    if (current.page) navigate(current.page);
    let raf = 0;
    const measure = () => {
      const el = findTarget(current.target);
      if (!el) return setBox(null);
      el.scrollIntoView({ block: "nearest" });
      const r = el.getBoundingClientRect();
      setBox({ x: r.left, y: r.top, w: r.width, h: r.height });
    };
    raf = requestAnimationFrame(() => requestAnimationFrame(measure));
    window.addEventListener("resize", measure);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", measure);
    };
  }, [current, navigate]);

  // Put the card beside the spotlight, kept inside the window.
  useLayoutEffect(() => {
    const c = card.current;
    if (!c || !current) return;
    const cw = c.offsetWidth;
    const ch = c.offsetHeight;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    if (!box) return setCardPos({ left: (vw - cw) / 2, top: (vh - ch) / 2 });
    const gap = 16;
    let left = box.x + box.w + gap;
    let top = box.y + box.h / 2 - ch / 2;
    if (left + cw > vw - 12) left = box.x - gap - cw; // sidebar on the right
    if (left < 12 || box.y + box.h < 80) {
      // Wide target, or one in the title bar (the search box): go below it.
      left = box.x + box.w / 2 - cw / 2;
      top = box.y + box.h + gap;
    }
    setCardPos({ left: Math.min(Math.max(12, left), vw - cw - 12), top: Math.min(Math.max(12, top), vh - ch - 12) });
  }, [box, current, step]);

  useEffect(() => {
    if (step === null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") finish();
      else if (e.key === "ArrowRight" || e.key === "Enter") setStep((s) => (s === null ? s : s + 1 < TOUR_STEPS.length ? s + 1 : (finish(), null)));
      else if (e.key === "ArrowLeft") setStep((s) => (s ? s - 1 : s));
      else return;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [step, finish]);

  useEffect(() => {
    if (step !== null) card.current?.querySelector<HTMLButtonElement>(".tour-next")?.focus();
  }, [step]);

  if (step === null || !current) return null;
  const last = step === TOUR_STEPS.length - 1;
  const pad = 6;
  return (
    <div className={`tour ${reduce ? "still" : ""}`} role="dialog" aria-modal="true" aria-labelledby="tour-title">
      {/* One spotlight for every step; with no target it covers
          the whole window and dims it. */}
      <div
        className={`tour-spot ${box ? "" : "none"}`}
        style={box ? { left: box.x - pad, top: box.y - pad, width: box.w + pad * 2, height: box.h + pad * 2 } : { left: 0, top: 0, width: "100%", height: "100%" }}
      />
      <div ref={card} className="tour-card" style={cardPos ? { left: cardPos.left, top: cardPos.top } : { visibility: "hidden" }} key={step}>
        <div className="tour-count">{step === 0 ? "Quick tour" : `${step} of ${TOUR_STEPS.length - 1}`}</div>
        <h2 id="tour-title">{current.title}</h2>
        <p>{current.body}</p>
        <div className="tour-dots" aria-hidden>
          {TOUR_STEPS.map((_, i) => <i key={i} className={i === step ? "on" : i < step ? "seen" : ""} />)}
        </div>
        <div className="row between" style={{ marginTop: 14 }}>
          {last ? <span /> : <button className="btn ghost sm" onClick={finish}>{step === 0 ? "Not now" : "Skip tour"}</button>}
          <div className="row" style={{ gap: 8 }}>
            {step > 0 && !last && <button className="btn ghost sm" onClick={() => setStep(step - 1)}>Back</button>}
            <button className="btn primary sm tour-next" onClick={() => (last ? finish() : setStep(step + 1))}>
              {step === 0 ? "Show me around" : last ? "Start using NOCTURN" : "Next"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
