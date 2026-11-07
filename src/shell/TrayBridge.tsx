// Keeps the tray menu in step with music and workspaces, and carries out
// what's picked there. System changes still go through the approval dialog.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import { useApp } from "../lib/app";
import { useMusic } from "../music/MusicProvider";
import { openPalette } from "./CommandPalette";
import { openReport } from "../components/Report";
import { openSlowCheck } from "../components/SlowCheck";
import { usePrefs } from "./PrefsProvider";
import { useWorkspaces } from "./WorkspaceProvider";

export function TrayBridge() {
  const m = useMusic();
  const { prefs, loaded } = usePrefs();
  const { requestApply } = useApp();
  const { switchTo } = useWorkspaces();

  const track = m.current ? `${m.current.title || "Untitled"} — ${m.current.artist || "Unknown artist"}` : null;
  const wsKey = JSON.stringify(prefs.workspaces.list.map((w) => [w.id, w.name, w.id === prefs.workspaces.active]));

  useEffect(() => {
    if (!loaded) return;
    const workspaces = prefs.workspaces.list.map((w) => ({ id: w.id, name: w.name, active: w.id === prefs.workspaces.active }));
    invoke("tray_update", { state: { track, playing: m.playing, workspaces } }).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded, track, m.playing, wsKey]);

  // Latest handlers without re-subscribing on every render.
  const h = useRef({ m, requestApply, switchTo, prefs });
  h.current = { m, requestApply, switchTo, prefs };

  useEffect(() => {
    const un = listen<string>("tray", (e) => {
      const { m, requestApply, switchTo, prefs } = h.current;
      const id = e.payload;
      if (id === "music:toggle") m.toggle();
      else if (id === "music:next") m.next();
      else if (id === "music:prev") m.prev();
      else if (id === "search") openPalette();
      else if (id === "report") openReport();
      else if (id === "slow") openSlowCheck();
      else if (id === "clean") requestApply({ tweakIds: ["performance.clean_temp"], source: "user", label: "Clean temporary files", title: "Clean temporary files" });
      else if (id === "ws:leave") switchTo(null);
      else if (id.startsWith("ws:")) {
        const ws = prefs.workspaces.list.find((w) => w.id === id.slice(3));
        if (ws) switchTo(ws.id);
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  return null;
}
