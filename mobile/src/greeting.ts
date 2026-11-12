// The greeting at the top of Home, which you can change in Settings › Home Screen.
import type { Library } from "./library";

export type DayPart = "morning" | "afternoon" | "evening" | "night";

export type HomePrefs = {
  /** added after the greeting: "Good evening, Sam" */
  name: string;
  /** "time" = changes with the time of day, "custom" = always your own message */
  mode: "time" | "custom";
  custom: string;
  /** your own words for each part of the day (blank = the usual one) */
  morning: string;
  afternoon: string;
  evening: string;
  night: string;
  /** the small line above the greeting (blank = "Nocturn Music") */
  eyebrow: string;
  moon: boolean;
  stars: boolean;
  big: boolean;
  /** what Home shows under the greeting */
  recent: boolean;
  picks: boolean;
};

export const DAY_PARTS: { id: DayPart; label: string; usual: string; hours: string }[] = [
  { id: "morning", label: "Morning", usual: "Good morning", hours: "5 am – noon" },
  { id: "afternoon", label: "Afternoon", usual: "Good afternoon", hours: "noon – 6 pm" },
  { id: "evening", label: "Evening", usual: "Good evening", hours: "6 pm – midnight" },
  { id: "night", label: "Late Night", usual: "Late night", hours: "midnight – 5 am" },
];

export const defaultHome: HomePrefs = {
  name: "",
  mode: "time",
  custom: "",
  morning: "",
  afternoon: "",
  evening: "",
  night: "",
  eyebrow: "",
  moon: true,
  stars: true,
  big: false,
  recent: true,
  picks: true,
};

export function homePrefs(lib: Library): HomePrefs {
  return { ...defaultHome, ...lib.home };
}

export function dayPart(d = new Date()): DayPart {
  const h = d.getHours();
  if (h < 5) return "night";
  if (h < 12) return "morning";
  if (h < 18) return "afternoon";
  return "evening";
}

export function greetingText(p: HomePrefs, d = new Date()): string {
  let line: string;
  if (p.mode === "custom" && p.custom.trim()) {
    line = p.custom.trim();
  } else {
    const part = dayPart(d);
    line = p[part].trim() || DAY_PARTS.find((x) => x.id === part)!.usual;
  }
  const name = p.name.trim();
  return name ? `${line}, ${name}` : line;
}

export function eyebrowText(p: HomePrefs): string {
  return p.eyebrow.trim() || "Nocturn Music";
}
