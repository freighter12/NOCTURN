// Ctrl+K command palette: jump to any page, run quick actions, open Library
// apps, switch workspaces, play music, change theme, or ask the AI.
// Anything that changes the system goes through the usual approval dialog.

import { useCallback, useEffect, useMemo, useRef, useState, type ComponentType } from "react";
import { I } from "../components/Icons";
import { api, library, type GameView, type LibraryItem } from "../lib/api";
import { launchGame } from "../library/games";
import { openReport } from "../components/Report";
import { useApp, type PageId } from "../lib/app";
import { launchItem, useAppIcon } from "../library/tiles";
import { useMusic } from "../music/MusicProvider";
import { pushRecent, rank, type Command } from "./commands";
import { ALL_PAGES, sectionOf } from "./nav";
import { usePrefs } from "./PrefsProvider";
import { previewIntro } from "./splash";
import { OFFICIAL_THEMES } from "./theme";
import { useWorkspaces } from "./WorkspaceProvider";

function CommandIcon({ c, Icon }: { c: Command; Icon: ComponentType | null }) {
  const real = useAppIcon(c.appId);
  if (real) {
    return (
      <span className="pi-icon real">
        <img src={real} alt="" draggable={false} />
      </span>
    );
  }
  return (
    <span className="pi-icon" style={c.badge?.colour ? { background: c.badge.colour, color: "#fff" } : undefined}>
      {Icon ? <Icon /> : c.badge ? c.badge.text : null}
    </span>
  );
}

/** Open the palette from anywhere (e.g. the title-bar button). */
export const openPalette = () => window.dispatchEvent(new Event("nocturn:palette"));

const RECENT_KEY = "nocturn.palette.recent";
const readRecent = (): string[] => {
  try {
    const v = JSON.parse(window.localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
  } catch {
    return [];
  }
};
const writeRecent = (ids: string[]) => {
  try {
    window.localStorage.setItem(RECENT_KEY, JSON.stringify(ids));
  } catch {
    /* per-device convenience only */
  }
};

const PAGE_WORDS: Partial<Record<PageId, string[]>> = {
  home: ["dashboard", "start"],
  library: ["apps", "launcher", "programs"],
  music: ["songs", "player", "playlists"],
  assistant: ["ai", "chat", "help", "ask"],
  gaming: ["games", "fps", "game mode"],
  benchmarks: ["speed test", "performance"],
  overview: ["health", "summary"],
  tweak: ["optimise", "optimize", "tweaks", "performance"],
  hardware: ["cpu", "gpu", "temperature", "ram"],
  processes: ["task manager", "kill", "end task", "cpu usage"],
  diagnostics: ["crashes", "drivers", "problems", "blue screen", "bsod"],
  network: ["internet", "wifi", "ping", "dns", "traceroute"],
  storage: ["disk", "space", "duplicates", "large files", "recycle bin"],
  startup: ["boot", "autostart", "login items"],
  services: ["windows services"],
  maintenance: ["schedule", "automatic"],
  profiles: ["power plan", "priority", "per app"],
  customise: ["taskbar", "file explorer", "notifications"],
  history: ["undo", "rollback", "journal", "changes"],
  studio: ["themes", "appearance", "accent", "layout", "experience studio"],
  settings: ["privacy", "api key", "preferences"],
};

const SUGGESTED = ["page:assistant", "action:clean-temp", "page:storage", "page:processes", "music:toggle", "page:studio"];

export function CommandPalette() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [apps, setApps] = useState<LibraryItem[]>([]);
  const [games, setGames] = useState<GameView[]>([]);
  const [recent, setRecent] = useState<string[]>(readRecent);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const returnFocus = useRef<HTMLElement | null>(null);

  const { navigate, requestApply, toast, settings } = useApp();
  const { prefs, update } = usePrefs();
  const { active: ws, switchTo } = useWorkspaces();
  const m = useMusic();

  const show = useCallback(() => {
    returnFocus.current = document.activeElement as HTMLElement | null;
    setQuery("");
    setActive(0);
    setRecent(readRecent());
    setOpen(true);
    library.list().then(setApps).catch(() => setApps([]));
    api.games(false).then(setGames).catch(() => setGames([]));
  }, []);
  const close = useCallback(() => {
    setOpen(false);
    returnFocus.current?.focus?.();
  }, []);

  // Ctrl+K anywhere (also while typing in a field — it's the one shortcut we own).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        if (open) close();
        else show();
      }
    };
    const onOpen = () => show();
    window.addEventListener("keydown", onKey);
    window.addEventListener("nocturn:palette", onOpen);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("nocturn:palette", onOpen);
    };
  }, [open, show, close]);

  useEffect(() => {
    if (open) requestAnimationFrame(() => input.current?.focus());
  }, [open]);

  const commands = useMemo<Command[]>(() => {
    if (!open) return [];
    const out: Command[] = [];
    const reduce = prefs.accessibility.reduceMotion || !!settings?.reduce_motion;

    for (const p of ALL_PAGES) {
      const sec = sectionOf(p.id);
      out.push({
        id: `page:${p.id}`,
        group: "Pages",
        label: p.label,
        hint: sec && sec.children ? sec.label : sec?.description,
        keywords: PAGE_WORDS[p.id],
        icon: p.icon,
        run: () => navigate(p.id),
      });
    }

    out.push(
      {
        id: "action:clean-temp",
        group: "Actions",
        label: "Clean temporary files",
        hint: "Shows what will be removed first",
        keywords: ["junk", "free space", "temp"],
        icon: "storage",
        run: () => requestApply({ tweakIds: ["performance.clean_temp"], source: "user", label: "Clean temporary files", title: "Clean temporary files" }),
      },
      { id: "action:benchmark", group: "Actions", label: "Run a benchmark", keywords: ["test speed"], icon: "bench", run: () => navigate("benchmarks") },
      { id: "action:crashes", group: "Actions", label: "Check for crashes and problems", keywords: ["diagnose", "why", "bsod"], icon: "diagnostics", run: () => navigate("diagnostics") },
      { id: "action:report", group: "Actions", label: "Report a problem", hint: "Make a report to send", keywords: ["bug", "feedback", "help", "logs", "support", "not working"], icon: "diagnostics", run: () => openReport() },
      { id: "action:undo", group: "Actions", label: "Undo a change", hint: "History", keywords: ["rollback", "revert"], icon: "undo", run: () => navigate("history") },
      { id: "action:dashboard", group: "Actions", label: "Edit the Home dashboard", keywords: ["widgets"], icon: "grid", run: () => navigate("studio") },
      { id: "action:intro", group: "Actions", label: "Replay the startup animation", keywords: ["splash", "intro", "moon"], icon: "studio", run: () => previewIntro(prefs.appearance.intro === "off" ? "full" : prefs.appearance.intro, reduce) },
    );

    for (const a of apps) {
      out.push({
        id: `app:${a.id}`,
        group: "Apps",
        label: a.name,
        hint: a.kind === "website" ? "Website" : a.category,
        keywords: [a.category, "open", "launch"],
        badge: { text: a.icon.glyph || a.name.replace(/[^\p{L}\p{N}]/gu, "").slice(0, 2) || "?", colour: a.icon.colour },
        appId: a.kind === "installed" && !a.icon.glyph && !a.icon.colour ? a.target : undefined,
        run: () => void launchItem(a, toast),
      });
    }

    for (const g of games) {
      if (g.not_game || !g.launch_via) continue;
      out.push({
        id: `game:${g.id}`,
        group: "Games",
        label: `Play ${g.name}`,
        hint: g.launcher,
        keywords: [g.name, "game", "launch", g.launcher],
        icon: "gaming",
        run: () => void launchGame(g, toast),
      });
    }

    for (const w of prefs.workspaces.list) {
      out.push({
        id: `ws:${w.id}`,
        group: "Workspaces",
        label: w.id === ws?.id ? `${w.name} (active)` : `Switch to ${w.name}`,
        keywords: [w.name, "workspace"],
        badge: { text: w.glyph || w.name.slice(0, 1), colour: w.accent },
        run: () => switchTo(w.id),
      });
    }
    if (ws) out.push({ id: "ws:leave", group: "Workspaces", label: `Leave ${ws.name}`, keywords: ["exit workspace", "no workspace"], icon: "workspaces", run: () => switchTo(null) });

    if (m.current) {
      out.push(
        { id: "music:toggle", group: "Music", label: m.playing ? "Pause" : "Play", hint: `${m.current.title} — ${m.current.artist}`, keywords: ["music", "resume", "pause", "play"], icon: m.playing ? "pause" : "play", run: () => m.toggle() },
        { id: "music:next", group: "Music", label: "Next track", keywords: ["skip"], icon: "next", run: () => m.next() },
        { id: "music:prev", group: "Music", label: "Previous track", keywords: ["back"], icon: "prev", run: () => m.prev() },
        { id: "music:shuffle", group: "Music", label: m.shuffle ? "Turn shuffle off" : "Turn shuffle on", icon: "shuffle", run: () => m.setShuffle(!m.shuffle) },
      );
    }
    const liked = m.tracks.filter((t) => t.liked).map((t) => t.id);
    if (liked.length) out.push({ id: "music:liked", group: "Music", label: "Play liked songs", hint: `${liked.length} songs`, keywords: ["favourites", "hearts"], icon: "heart", run: () => m.play(liked) });
    for (const p of m.playlists) {
      const ids = p.tracks.filter((id) => m.byId.has(id));
      if (!ids.length) continue;
      out.push({ id: `music:pl:${p.id}`, group: "Music", label: `Play ${p.name}`, hint: `Playlist · ${ids.length} songs`, keywords: [p.name, "playlist"], icon: "music", run: () => m.play(ids) });
    }

    for (const t of [...OFFICIAL_THEMES, ...prefs.appearance.customThemes]) {
      if (t.id === prefs.appearance.theme) continue;
      out.push({
        id: `theme:${t.id}`,
        group: "Themes",
        label: `Theme: ${t.name}`,
        hint: t.kind === "light" ? "Light" : "Dark",
        keywords: [t.name, "theme", "colours", t.kind],
        icon: "studio",
        run: () => update((p) => ({ ...p, appearance: { ...p.appearance, theme: t.id } })),
      });
    }

    out.push(
      {
        id: "set:reduce-motion",
        group: "Settings",
        label: prefs.accessibility.reduceMotion ? "Turn reduce motion off" : "Turn reduce motion on",
        keywords: ["animations", "accessibility"],
        icon: "settings",
        run: () => update((p) => ({ ...p, accessibility: { ...p.accessibility, reduceMotion: !p.accessibility.reduceMotion } })),
      },
      {
        id: "set:compact",
        group: "Settings",
        label: prefs.navigation.compact ? "Show the full sidebar" : "Make the sidebar compact",
        keywords: ["sidebar", "navigation", "collapse"],
        icon: "settings",
        run: () => update((p) => ({ ...p, navigation: { ...p.navigation, compact: !p.navigation.compact } })),
      },
    );
    return out;
  }, [open, apps, games, prefs, settings?.reduce_motion, ws, m, navigate, requestApply, toast, switchTo, update]);

  const results = useMemo(() => {
    const r = rank(query, commands, recent, { suggestions: SUGGESTED });
    const q = query.trim();
    if (q) {
      r.push({ id: "ask", group: "Ask", label: `Ask Nocturn AI: “${q.length > 60 ? q.slice(0, 60) + "…" : q}”`, icon: "assistant", run: () => navigate("assistant", { prompt: q }) });
    }
    return r;
  }, [query, commands, recent, navigate]);

  useEffect(() => setActive(0), [query]);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const run = (c: Command) => {
    if (c.id !== "ask") {
      const next = pushRecent(recent, c.id);
      setRecent(next);
      writeRecent(next);
    }
    setOpen(false);
    // Let the palette unmount before the command opens a dialog of its own.
    requestAnimationFrame(() => c.run());
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" || (e.key === "Tab" && !e.shiftKey)) {
      e.preventDefault();
      setActive((i) => (results.length ? (i + 1) % results.length : 0));
    } else if (e.key === "ArrowUp" || (e.key === "Tab" && e.shiftKey)) {
      e.preventDefault();
      setActive((i) => (results.length ? (i - 1 + results.length) % results.length : 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const c = results[active];
      if (c) run(c);
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  };

  if (!open) return null;
  let lastGroup = "";
  return (
    <div className="palette-scrim" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="palette" role="dialog" aria-modal="true" aria-label="Command palette" onKeyDown={onKeyDown}>
        <div className="palette-input">
          <I.search />
          <input
            ref={input}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search pages, actions, apps, music…"
            aria-label="Search commands"
            role="combobox"
            aria-expanded="true"
            aria-controls="palette-list"
            aria-activedescendant={results[active] ? `pal-${active}` : undefined}
            spellCheck={false}
          />
          <kbd>Esc</kbd>
        </div>
        <div className="palette-list" id="palette-list" role="listbox" ref={list}>
          {results.length === 0 && <div className="palette-empty">Nothing matches. Try a page name, an app, or a song.</div>}
          {results.map((c, i) => {
            const heading = c.group !== lastGroup ? c.group : null;
            lastGroup = c.group;
            const Icon = (c.icon && c.icon in I ? I[c.icon as keyof typeof I] : null) as ComponentType | null;
            return (
              <div key={`${c.group}:${c.id}`}>
                {heading && <div className="palette-group">{heading === "Ask" ? "Nocturn AI" : heading}</div>}
                <div
                  id={`pal-${i}`}
                  data-i={i}
                  role="option"
                  aria-selected={i === active}
                  className={"palette-item" + (i === active ? " active" : "")}
                  onMouseMove={() => i !== active && setActive(i)}
                  onClick={() => run(c)}
                >
                  <CommandIcon c={c} Icon={Icon} />
                  <span className="pi-label">{c.label}</span>
                  {c.hint && <span className="pi-hint">{c.hint}</span>}
                  {i === active && <span className="pi-enter" aria-hidden>↵</span>}
                </div>
              </div>
            );
          })}
        </div>
        <div className="palette-foot">
          <span><kbd>↑</kbd><kbd>↓</kbd> move</span>
          <span><kbd>↵</kbd> open</span>
          <span><kbd>Ctrl</kbd><kbd>K</kbd> toggle</span>
        </div>
      </div>
    </div>
  );
}
