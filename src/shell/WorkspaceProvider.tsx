// Switching workspaces: applies the workspace's look immediately and opens
// its apps — after showing which ones, unless the user turned that off.

import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { errorMessage, library, type LibraryItem } from "../lib/api";
import { useApp } from "../lib/app";
import { AppIcon, whereItOpens } from "../library/tiles";
import { usePrefs } from "./PrefsProvider";
import { useMusic } from "../music/MusicProvider";
import type { Workspace } from "./prefs";

interface Ctx {
  active: Workspace | null;
  /** Switch to a workspace (null = leave workspaces). */
  switchTo: (id: string | null) => void;
}

const WsCtx = createContext<Ctx | null>(null);

export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const { prefs, loaded, update } = usePrefs();
  const { toast } = useApp();
  const m = useMusic();
  const [pending, setPending] = useState<{ ws: Workspace; items: LibraryItem[] } | null>(null);
  const [playMusic, setPlayMusic] = useState(true);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [dontAsk, setDontAsk] = useState(false);
  const active = prefs.workspaces.list.find((w) => w.id === prefs.workspaces.active) ?? null;

  // Start in the chosen workspace (its look only — apps aren't opened at startup).
  const started = useRef(false);
  useEffect(() => {
    if (loaded && !started.current) {
      started.current = true;
      const sw = prefs.workspaces.startWith;
      if (sw && sw !== prefs.workspaces.active) update((p) => ({ ...p, workspaces: { ...p.workspaces, active: sw } }));
    }
  }, [loaded, prefs.workspaces.startWith, prefs.workspaces.active, update]);

  const launchAll = useCallback(
    async (ws: Workspace, items: LibraryItem[]) => {
      let ok = 0;
      const failed: string[] = [];
      for (const it of items) {
        try {
          await library.launch(it.id);
          ok++;
        } catch (e) {
          failed.push(`${it.name} (${errorMessage(e)})`);
        }
        // A short gap so a group of apps doesn't all start at the same instant.
        await new Promise((r) => setTimeout(r, 350));
      }
      toast({
        tone: failed.length ? "warning" : "good",
        title: `${ws.name}: opened ${ok} app${ok === 1 ? "" : "s"}`,
        detail: failed.length ? `Couldn't open ${failed.join(", ")}` : "They opened in their own windows.",
      });
    },
    [toast],
  );

  const switchTo = useCallback(
    async (id: string | null) => {
      update((p) => ({ ...p, workspaces: { ...p.workspaces, active: id } }));
      const ws = prefs.workspaces.list.find((w) => w.id === id);
      if (!ws) {
        toast({ tone: "info", title: "Workspaces off", detail: "Back to your main Nocturn setup." });
        return;
      }
      const playlist = ws.playlist ? m.playlists.find((p) => p.id === ws.playlist) : undefined;
      const all = ws.launch.length ? await library.list().catch(() => [] as LibraryItem[]) : [];
      const items = ws.launch.map((x) => all.find((i) => i.id === x)).filter((i): i is LibraryItem => !!i);
      if (!items.length && !playlist?.tracks.length) {
        toast({ tone: "info", title: `Switched to ${ws.name}` });
        return;
      }
      if (prefs.workspaces.confirmLaunch) {
        setPicked(new Set(items.map((i) => i.id)));
        setDontAsk(false);
        setPlayMusic(true);
        setPending({ ws, items });
      } else {
        if (playlist?.tracks.length) m.play(playlist.tracks);
        if (items.length) launchAll(ws, items);
      }
    },
    [prefs.workspaces, update, toast, launchAll, m],
  );

  return (
    <WsCtx.Provider value={{ active, switchTo }}>
      {children}
      {pending && (
        <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && setPending(null)}>
          <div className="modal" role="dialog" aria-modal="true" aria-labelledby="ws-launch-title" style={{ maxWidth: 520 }}>
            <div className="modal-head">
              <h2 id="ws-launch-title">Start {pending.ws.name}?</h2>
              <p>Apps open in their own windows. Untick anything you don't need right now.</p>
            </div>
            <div className="modal-body">
              <div className="stack" style={{ gap: 6 }}>
                {(() => {
                  const pl = pending.ws.playlist ? m.playlists.find((p) => p.id === pending.ws.playlist) : undefined;
                  return pl ? (
                    <label className="pick">
                      <input type="checkbox" className="check" checked={playMusic} onChange={(e) => setPlayMusic(e.target.checked)} />
                      <span className="art art-empty" style={{ width: 28, height: 28 }}>♪</span>
                      <span>Play {pl.name}</span>
                      <span className="small dim" style={{ marginLeft: "auto" }}>Nocturn Music</span>
                    </label>
                  ) : null;
                })()}
                {pending.items.map((i) => (
                  <label key={i.id} className="pick">
                    <input type="checkbox" className="check" checked={picked.has(i.id)} onChange={() => setPicked((p) => { const n = new Set(p); n.has(i.id) ? n.delete(i.id) : n.add(i.id); return n; })} />
                    <AppIcon item={i} size={28} />
                    <span>{i.name}</span>
                    <span className="small dim" style={{ marginLeft: "auto" }}>{whereItOpens(i)}</span>
                  </label>
                ))}
              </div>
            </div>
            <div className="modal-foot">
              <label className="row left small" style={{ gap: 8 }}>
                <input type="checkbox" className="check" checked={dontAsk} onChange={(e) => setDontAsk(e.target.checked)} />
                Don't ask again
              </label>
              <button className="btn ghost" onClick={() => setPending(null)}>Not now</button>
              <button
                className="btn primary"
                disabled={picked.size === 0 && !playMusic}
                onClick={() => {
                  if (dontAsk) update((p) => ({ ...p, workspaces: { ...p.workspaces, confirmLaunch: false } }));
                  const p = pending;
                  setPending(null);
                  const pl = p.ws.playlist ? m.playlists.find((x) => x.id === p.ws.playlist) : undefined;
                  if (playMusic && pl?.tracks.length) m.play(pl.tracks);
                  const chosen = p.items.filter((i) => picked.has(i.id));
                  if (chosen.length) launchAll(p.ws, chosen);
                }}
              >
                Start
              </button>
            </div>
          </div>
        </div>
      )}
    </WsCtx.Provider>
  );
}

export function useWorkspaces() {
  const c = useContext(WsCtx);
  if (!c) throw new Error("useWorkspaces outside WorkspaceProvider");
  return c;
}
