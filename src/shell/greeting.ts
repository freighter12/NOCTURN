// The Home greeting: "Good evening, Sam" by default, or the user's own words
// with {greeting} and {name} filled in.

import type { Greeting } from "./prefs";

export function timeGreeting(d = new Date()) {
  const h = d.getHours();
  return h < 5 ? "Good night" : h < 12 ? "Good morning" : h < 18 ? "Good afternoon" : "Good evening";
}

export const DEFAULT_SUBTITLE = "Your PC at a glance. Choose what appears here in Customise.";

export function greetingTitle(g: Greeting, d = new Date()) {
  const base = timeGreeting(d);
  const name = g.name.trim();
  if (!g.text.trim()) return name ? `${base}, ${name}` : base;
  return g.text
    .replace(/\{greeting\}/gi, base)
    .replace(/\{name\}/gi, name)
    // "{greeting}, {name}" with no name shouldn't leave a dangling comma.
    .replace(/\s*,\s*([!.?]?)\s*$/, "$1")
    .replace(/\s{2,}/g, " ")
    .trim();
}

export function greetingSubtitle(g: Greeting) {
  return g.subtitle.trim() || DEFAULT_SUBTITLE;
}
