// Shared pieces of the App Library: Nocturn's own apps, app icons and the
// launch action (with an honest message about where the app opened).

import { useEffect, useState } from "react";
import { I } from "../components/Icons";
import { errorMessage, library, type LibraryItem } from "../lib/api";
import type { PageId } from "../lib/app";

/** Nocturn's own apps — open inside Nocturn. */
export const NOCTURN_APPS: { page: PageId; name: string; description: string; icon: keyof typeof I }[] = [
  { page: "music", name: "Nocturn Music", description: "Your music, no ads", icon: "music" },
  { page: "assistant", name: "Nocturn AI", description: "Ask about your PC", icon: "assistant" },
  { page: "tweak", name: "Optimiser", description: "Reversible Windows tweaks", icon: "tweak" },
  { page: "hardware", name: "System Monitor", description: "Live hardware readings", icon: "chip" },
  { page: "network", name: "Network Tools", description: "Latency, DNS, traceroute", icon: "network" },
  { page: "storage", name: "Storage Tools", description: "What's using your space", icon: "storage" },
  { page: "benchmarks", name: "Benchmarks", description: "Measure before and after", icon: "bench" },
  { page: "workspaces", name: "Workspaces", description: "Environments for activities", icon: "workspaces" },
];

// Muted colours for monograms, picked by name so an app keeps its colour.
const PALETTE = ["#3b4a6b", "#4a3b6b", "#3b6b5c", "#6b553b", "#6b3b4a", "#3b5f6b", "#5c6b3b", "#55606e"];

function hash(s: string) {
  let h = 0;
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) | 0;
  return Math.abs(h);
}

// ------------------------------------------------------------ real icons

// Icons Windows gave us, by Start-menu AppID (null = it has none).
const iconCache = new Map<string, string | null>();
const waiting = new Map<string, ((v: string | null) => void)[]>();
let flushTimer: number | undefined;

function flush() {
  flushTimer = undefined;
  const ids = [...waiting.keys()].filter((id) => !iconCache.has(id));
  for (let i = 0; i < ids.length; i += 100) {
    const batch = ids.slice(i, i + 100);
    library
      .icons(batch)
      .then((map) => batch.forEach((id) => iconCache.set(id, map[id] ?? null)))
      .catch(() => batch.forEach((id) => iconCache.set(id, null)))
      .finally(() =>
        batch.forEach((id) => {
          const ws = waiting.get(id) ?? [];
          waiting.delete(id);
          ws.forEach((f) => f(iconCache.get(id) ?? null));
        }),
      );
  }
}

/** The app's own icon (data URL), null if it has none, undefined while loading. Requests are batched. */
export function useAppIcon(appId: string | undefined): string | null | undefined {
  const [src, setSrc] = useState<string | null | undefined>(() => (appId ? iconCache.get(appId) : undefined));
  useEffect(() => {
    if (!appId) return setSrc(undefined);
    if (iconCache.has(appId)) return setSrc(iconCache.get(appId));
    let live = true;
    setSrc(undefined);
    const list = waiting.get(appId) ?? [];
    list.push((v) => live && setSrc(v));
    waiting.set(appId, list);
    flushTimer ??= window.setTimeout(flush, 16);
    return () => {
      live = false;
    };
  }, [appId]);
  return src;
}

type IconItem = Pick<LibraryItem, "name" | "icon"> & Partial<Pick<LibraryItem, "kind" | "target">>;

/**
 * An app's tile icon: its real Windows icon for installed apps (unless the
 * user chose their own text/colour), otherwise a monogram. `appId` gives
 * the Start-menu ID directly (e.g. in the Add apps picker).
 */
export function AppIcon({ item, size = 44, appId }: { item: IconItem; size?: number; appId?: string }) {
  const custom = !!(item.icon.glyph || item.icon.colour);
  const id = custom ? undefined : (appId ?? (item.kind === "installed" ? item.target : undefined));
  const real = useAppIcon(id);
  if (real) {
    return (
      <span className="app-icon real" style={{ width: size, height: size }} aria-hidden>
        <img src={real} alt="" draggable={false} style={{ width: Math.round(size * 0.74), height: Math.round(size * 0.74) }} />
      </span>
    );
  }
  const bg = item.icon.colour ?? PALETTE[hash(item.name) % PALETTE.length];
  const glyph = item.icon.glyph || item.name.replace(/[^\p{L}\p{N}]/gu, "").slice(0, 2) || "?";
  return (
    <span className="app-icon" style={{ background: bg, width: size, height: size, fontSize: size * 0.38 }} aria-hidden>
      {glyph}
    </span>
  );
}

export function NocturnIcon({ icon, size = 44 }: { icon: keyof typeof I; size?: number }) {
  const Icon = I[icon];
  return (
    <span className="app-icon nocturn" style={{ width: size, height: size }} aria-hidden>
      <Icon />
    </span>
  );
}

export function whereItOpens(i: LibraryItem): string {
  if (i.kind === "installed") return "Opens the app";
  if (i.kind === "website") return "Opens in your browser";
  return i.app_installed ? "Opens the app" : "Opens the website";
}

type Toast = (t: { tone: "good" | "warning" | "critical" | "info"; title: string; detail?: string }) => void;

export async function launchItem(item: LibraryItem, toast: Toast, onDone?: () => void, deepLink?: string) {
  try {
    const r = await library.launch(item.id, deepLink);
    toast({ tone: "good", title: r.detail, detail: r.via === "browser" ? "It opened outside Nocturn, in your default browser." : "It opened in its own window." });
    onDone?.();
  } catch (e) {
    toast({ tone: "critical", title: `Couldn't open ${item.name}`, detail: errorMessage(e) });
  }
}
