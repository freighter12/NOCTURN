// The navigation model. Sections are what the sidebar shows; each opens one
// page or groups several. Every page that existed before the ecosystem
// expansion keeps its ID — only where it's listed changes.

import type { I } from "../components/Icons";
import type { PageId } from "../lib/app";

export interface NavPage {
  id: PageId;
  label: string;
  icon: keyof typeof I;
}

export interface NavSection {
  id: string;
  label: string;
  icon: keyof typeof I;
  /** A section either opens one page… */
  page?: PageId;
  /** …or groups several (the first is opened when the section is clicked). */
  children?: NavPage[];
  description: string;
}

export const SECTIONS: NavSection[] = [
  { id: "home", label: "Home", icon: "overview", page: "home", description: "Your dashboard" },
  { id: "library", label: "Library", icon: "library", page: "library", description: "Apps and services" },
  { id: "music", label: "Music", icon: "music", page: "music", description: "Nocturn Music" },
  { id: "workspaces", label: "Workspaces", icon: "workspaces", page: "workspaces", description: "Environments for different activities" },
  { id: "ai", label: "AI", icon: "assistant", page: "assistant", description: "Nocturn AI" },
  {
    id: "gaming",
    label: "Gaming",
    icon: "gaming",
    description: "Games, sessions and benchmarks",
    children: [
      { id: "gaming", label: "Gaming", icon: "gaming" },
      { id: "benchmarks", label: "Benchmarks", icon: "bench" },
    ],
  },
  {
    id: "system",
    label: "System",
    icon: "tweak",
    description: "Optimiser, diagnostics and system tools",
    children: [
      { id: "overview", label: "Overview", icon: "chip" },
      { id: "tweak", label: "Optimiser", icon: "tweak" },
      { id: "hardware", label: "Hardware", icon: "chip" },
      { id: "processes", label: "Processes", icon: "processes" },
      { id: "diagnostics", label: "Diagnostics", icon: "diagnostics" },
      { id: "network", label: "Network", icon: "network" },
      { id: "storage", label: "Storage", icon: "storage" },
      { id: "startup", label: "Startup apps", icon: "startup" },
      { id: "services", label: "Services", icon: "services" },
      { id: "maintenance", label: "Maintenance", icon: "maintenance" },
      { id: "profiles", label: "App profiles", icon: "profiles" },
      { id: "customise", label: "Windows settings", icon: "customise" },
      { id: "history", label: "History", icon: "history" },
    ],
  },
  { id: "studio", label: "Customise", icon: "studio", page: "studio", description: "Experience Studio" },
  { id: "settings", label: "Settings", icon: "settings", page: "settings", description: "Accounts, privacy and app settings" },
];

export const SECTION_IDS = SECTIONS.map((s) => s.id);

/** Every page reachable from navigation. */
export const ALL_PAGES: NavPage[] = SECTIONS.flatMap((s) => (s.children ? s.children : [{ id: s.page!, label: s.label, icon: s.icon }]));
export const PAGE_IDS = ALL_PAGES.map((p) => p.id);

export function sectionOf(page: PageId): NavSection | undefined {
  return SECTIONS.find((s) => s.page === page || s.children?.some((c) => c.id === page));
}

export function pageLabel(page: PageId): string {
  return ALL_PAGES.find((p) => p.id === page)?.label ?? page;
}
