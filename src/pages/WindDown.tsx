// Wind-down: a calmer last half hour before bed — warmer screen, music
// fading down, quiet notifications — and, at bedtime, the screen off or the
// PC asleep. On a schedule or right now.

import { useEffect, useRef, useState } from "react";
import { I } from "../components/Icons";
import { Note, PageHead, Panel, Seg, Toggle } from "../components/ui";
import { winddown, type BedtimeAction, type WindDownConfig } from "../lib/api";
import { useMusic } from "../music/MusicProvider";
import { useSmartLists } from "../music/SmartLists";
import { clock, useWindDown } from "../shell/WindDownProvider";

const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const ACTIONS: { value: BedtimeAction; label: string; hint: string }[] = [
  { value: "nothing", label: "Nothing", hint: "Wind-down just ends with the music; the PC stays as it is." },
  { value: "screen_off", label: "Turn the screen off", hint: "The display goes dark; anything running keeps running. Move the mouse to wake it." },
  { value: "sleep", label: "Sleep the PC", hint: "After a one-minute warning you can cancel. Skipped while a game is running." },
];

function kelvin(strength: number) {
  return Math.round((6500 - strength * (6500 - 2700)) / 100) * 100;
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="studio-row">
      <div>
        <div className="studio-label">{label}</div>
        {hint && <div className="small dim">{hint}</div>}
      </div>
      <div className="studio-control" style={{ gap: 8, alignItems: "center", flexWrap: "wrap" }}>{children}</div>
    </div>
  );
}

/** "tonight", "tomorrow" or "on Friday". */
function dayWord(iso: string) {
  const d = new Date(iso);
  const days = Math.round((new Date(d.toDateString()).getTime() - new Date(new Date().toDateString()).getTime()) / 86_400_000);
  return days <= 0 ? "tonight" : days === 1 ? "tomorrow" : `on ${d.toLocaleDateString([], { weekday: "long" })}`;
}

function useCountdown(iso: string | null | undefined) {
  const [, tick] = useState(0);
  useEffect(() => {
    if (!iso) return;
    const iv = window.setInterval(() => tick((x) => x + 1), 1000);
    return () => window.clearInterval(iv);
  }, [iso]);
  return iso ? Math.max(0, Math.round((Date.parse(iso) - Date.now()) / 1000)) : null;
}

function Hero() {
  const w = useWindDown();
  const s = w.status;
  const sleepIn = useCountdown(s?.sleep_at);
  const [mins, setMins] = useState(30);
  if (!s) return <Panel><div className="small dim">Loading…</div></Panel>;
  const on = s.phase !== "off";
  const pct = Math.round(s.progress * 100);
  return (
    <section className={`panel wd-hero ${on ? "on" : ""}`} style={{ ["--wd" as string]: s.progress }}>
      <div className="wd-sky" aria-hidden>
        <span className="wd-moon" />
      </div>
      <div className="stack" style={{ gap: 6, position: "relative" }}>
        {s.phase === "off" && (
          <>
            <div className="eyebrow small dim">Not winding down</div>
            <h2 style={{ margin: 0 }}>Ready when you are</h2>
            <div className="muted">{s.next_start ? `Starts on its own ${dayWord(s.next_start)} at ${clock(s.next_start)}.` : "No schedule — start it yourself, or set a bedtime below."}</div>
            <div className="row mt-2" style={{ gap: 8, flexWrap: "wrap" }}>
              <button className="btn primary" onClick={() => w.start(mins)}><I.moon /> Wind down now</button>
              <select className="select" style={{ width: "auto" }} aria-label="Bedtime in" value={mins} onChange={(e) => setMins(Number(e.target.value))}>
                {[15, 30, 45, 60, 90].map((n) => <option key={n} value={n}>bedtime in {n} min</option>)}
                <option value={0}>bedtime now</option>
              </select>
            </div>
          </>
        )}
        {s.phase === "winding" && (
          <>
            <div className="eyebrow small dim">Winding down{s.manual ? "" : " · scheduled"}</div>
            <h2 style={{ margin: 0 }}>Bedtime at {clock(s.bedtime)}</h2>
            <div className="wd-progress" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100} aria-label="Wind-down progress"><span style={{ width: `${pct}%` }} /></div>
            <div className="small muted">
              {[s.config.warm && (s.warm_applied > 0 ? `screen at ${kelvin(s.warm_applied)} K` : "screen warming"), s.config.music_fade && "music fading", s.config.quiet && "notifications quiet"].filter(Boolean).join(" · ") || "Nothing set to change — see below."}
            </div>
            <div className="row mt-2" style={{ gap: 8 }}>
              <button className="btn" onClick={() => w.snooze(15)}>15 minutes more</button>
              <button className="btn ghost" onClick={w.stop}>Stop</button>
            </div>
          </>
        )}
        {s.phase === "night" && (
          <>
            <div className="eyebrow small dim">Bedtime</div>
            <h2 style={{ margin: 0 }}>Good night</h2>
            <div className="muted">The screen stays warm until {clock(s.morning)}.</div>
            {sleepIn != null && (
              <Note tone="warning">
                Your PC will sleep in {sleepIn} s.{" "}
                <button className="btn sm" style={{ marginLeft: 8 }} onClick={w.cancelSleep}>Cancel</button>
                <button className="btn ghost sm" onClick={() => w.snooze(15)}>15 minutes more</button>
              </Note>
            )}
            <div className="row mt-2" style={{ gap: 8 }}>
              <button className="btn ghost" onClick={w.stop}>Back to normal</button>
              {sleepIn == null && <button className="btn ghost" onClick={() => w.snooze(15)}>15 minutes more</button>}
            </div>
          </>
        )}
        {s.note && <div className="small dim mt-1">{s.note}</div>}
      </div>
    </section>
  );
}

export function WindDown() {
  const w = useWindDown();
  const m = useMusic();
  const smart = useSmartLists();
  const [c, setC] = useState<WindDownConfig | null>(null);
  const saveTimer = useRef(0);
  useEffect(() => {
    if (w.status && !c) setC(w.status.config);
  }, [w.status, c]);
  if (!c) return <div className="page"><PageHead eyebrow="Nocturn" title="Wind down" /><Hero /></div>;
  const set = (patch: Partial<WindDownConfig>) => {
    const next = { ...c, ...patch };
    setC(next);
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => w.save(next), 350);
  };
  const lists = [...smart.builtins.filter((l) => l.id === "builtin:winddown" || l.id === "builtin:time"), ...smart.custom];
  return (
    <div className="page">
      <PageHead eyebrow="Nocturn" title="Wind down" desc="A calmer end to the day: the screen warms, the music fades and notifications go quiet — then, at bedtime, lights out." />
      <Hero />
      <div className="grid cols-2 mt-2 wd-grid" style={{ alignItems: "start" }}>
        <Panel title="When" sub="Starts by itself before bedtime on the nights you pick.">
          <Row label="On a schedule">
            <Toggle on={c.scheduled} label="On a schedule" onChange={(v) => set({ scheduled: v })} />
          </Row>
          <Row label="Bedtime">
            <input className="input" type="time" aria-label="Bedtime" value={c.bedtime} onChange={(e) => e.target.value && set({ bedtime: e.target.value })} />
          </Row>
          <Row label="Wind down for" hint="How long before bedtime it starts.">
            <select className="select" style={{ width: "auto" }} aria-label="Wind down for" value={c.lead_min} onChange={(e) => set({ lead_min: Number(e.target.value) })}>
              {[10, 15, 20, 30, 45, 60, 90, 120].map((n) => <option key={n} value={n}>{n} minutes</option>)}
            </select>
          </Row>
          <Row label="Nights">
            <div className="chips wd-days">
              {DAYS.map((d, i) => (
                <button key={d} className={`chip ${c.days.includes(i) ? "on" : ""}`} aria-pressed={c.days.includes(i)} onClick={() => set({ days: c.days.includes(i) ? c.days.filter((x) => x !== i) : [...c.days, i].sort() })}>{d}</button>
              ))}
            </div>
          </Row>
          <Row label="Morning" hint="When the screen goes back to normal.">
            <input className="input" type="time" aria-label="Morning" value={c.morning} onChange={(e) => e.target.value && set({ morning: e.target.value })} />
          </Row>
        </Panel>
        <Panel title="Screen" sub="Warmer colours with less blue light, eased in over the wind-down.">
          <Row label="Warm the screen">
            <Toggle on={c.warm} label="Warm the screen" onChange={(v) => set({ warm: v })} />
          </Row>
          <Row label="Warmth" hint={`${kelvin(c.warm_strength)} K at bedtime (daylight is 6500 K).`}>
            <input type="range" className="range" style={{ width: 150 }} min={0.1} max={1} step={0.05} value={c.warm_strength} disabled={!c.warm} aria-label="Warmth" onChange={(e) => set({ warm_strength: Number(e.target.value) })} />
            <button className="btn ghost sm" disabled={!c.warm} onClick={() => winddown.previewWarm(c.warm_strength)} title="Shows this warmth for 3 seconds">Preview</button>
          </Row>
          <div className="small dim">Works like f.lux, through your display's colour settings. Some HDR screens and remote desktops ignore it.</div>
        </Panel>
        <Panel title="Music" sub="Nocturn Music plays you out.">
          <Row label="Fade the music down" hint={`To ${Math.round(c.music_floor * 100)}% of your volume by bedtime.`}>
            <input type="range" className="range" style={{ width: 120 }} min={0.1} max={1} step={0.05} value={c.music_floor} disabled={!c.music_fade} aria-label="Volume at bedtime" onChange={(e) => set({ music_floor: Number(e.target.value) })} />
            <Toggle on={c.music_fade} label="Fade the music down" onChange={(v) => set({ music_fade: v })} />
          </Row>
          <Row label="Stop at bedtime" hint="A slow 30-second fade, then pause.">
            <Toggle on={c.music_stop} label="Stop at bedtime" onChange={(v) => set({ music_stop: v })} />
          </Row>
          <Row label="Put something on" hint="When wind-down starts and nothing is playing.">
            <select className="select" aria-label="Playlist" style={{ maxWidth: 220 }} value={c.playlist ?? ""} onChange={(e) => set({ playlist: e.target.value || null })}>
              <option value="">Nothing</option>
              {lists.length > 0 && <optgroup label="Smart">{lists.map((l) => <option key={l.id} value={l.id}>{l.name}</option>)}</optgroup>}
              {m.playlists.length > 0 && <optgroup label="Playlists">{m.playlists.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}</optgroup>}
              {c.playlist && !lists.some((l) => l.id === c.playlist) && !m.playlists.some((p) => p.id === c.playlist) && <option value={c.playlist}>(a list that's gone)</option>}
            </select>
          </Row>
        </Panel>
        <Panel title="Quiet & bedtime">
          <Row label="Quiet notifications" hint="NOCTURN's own notifications wait until morning.">
            <Toggle on={c.quiet} label="Quiet notifications" onChange={(v) => set({ quiet: v })} />
          </Row>
          <div className="studio-label mt-2">At bedtime</div>
          <div className="mt-1"><Seg value={c.bedtime_action} onChange={(v) => set({ bedtime_action: v })} options={ACTIONS.map(({ value, label }) => ({ value, label }))} /></div>
          <div className="small dim mt-1">{ACTIONS.find((a) => a.value === c.bedtime_action)?.hint}</div>
        </Panel>
      </div>
    </div>
  );
}
