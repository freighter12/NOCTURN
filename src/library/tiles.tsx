// Shared pieces of the App Library: Nocturn's own apps, app icons and the
// launch action (with an honest message about where the app opened).

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

export function AppIcon({ item, size = 44 }: { item: Pick<LibraryItem, "name" | "icon">; size?: number }) {
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
