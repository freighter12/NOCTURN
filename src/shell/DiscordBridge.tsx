// Shows the playing song on the user's Discord profile (opt-in, Customise ›
// Music). Updates only when something a viewer would notice changes: the
// song, play/pause, or a seek — not every second.

import { useEffect, useRef, useState } from "react";
import { discord, type DiscordPresence } from "../lib/api";
import { useMusic } from "../music/MusicProvider";
import { usePrefs } from "./PrefsProvider";

export function DiscordBridge() {
  const m = useMusic();
  const { prefs, loaded } = usePrefs();
  const mp = prefs.music;
  const last = useRef<string>("");
  const t = m.current;

  // A seek moves the song's start time; notice jumps of more than 2 s.
  const [seeks, setSeeks] = useState(0);
  const startRef = useRef(0);
  useEffect(() => {
    const start = Date.now() - m.position;
    if (m.playing && Math.abs(start - startRef.current) > 2000) setSeeks((n) => n + 1);
    startRef.current = start;
  }, [m.position, m.playing]);

  useEffect(() => {
    if (!loaded) return;
    let p: DiscordPresence | null = null;
    if (mp.discord && t && (m.playing || mp.discordWhenPaused)) {
      const dur = m.duration || t.duration_ms;
      const start = Date.now() - m.timeMs();
      p = {
        title: t.title,
        artist: t.artist,
        album: t.album,
        start_ms: mp.discordProgress && m.playing ? Math.round(start) : null,
        end_ms: mp.discordProgress && m.playing && dur > 0 ? Math.round(start + dur) : null,
        paused: !m.playing,
      };
    }
    // Compare without the exact timestamps (they drift by milliseconds).
    const key = JSON.stringify(p && { ...p, start_ms: null, end_ms: null, id: t?.id, seeks });
    if (key === last.current) return;
    last.current = key;
    discord.presence(p, t?.id ?? null).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded, mp.discord, mp.discordProgress, mp.discordWhenPaused, t?.id, m.playing, m.duration, seeks]);

  // Clear it when the app closes the page.
  useEffect(() => () => void discord.presence(null, null).catch(() => {}), []);
  return null;
}
