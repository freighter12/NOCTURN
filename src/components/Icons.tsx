// Minimal 16px line icons (1.5px strokes) drawn for NOCTURN.
import type { SVGProps } from "react";

type P = SVGProps<SVGSVGElement>;
const base = (p: P) => ({
  viewBox: "0 0 16 16",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
  ...p,
});

export const I = {
  overview: (p: P) => (
    <svg {...base(p)}><rect x="2" y="2" width="5" height="6" rx="1.2" /><rect x="9" y="2" width="5" height="3.5" rx="1.2" /><rect x="9" y="7.5" width="5" height="6.5" rx="1.2" /><rect x="2" y="10" width="5" height="4" rx="1.2" /></svg>
  ),
  chip: (p: P) => (
    <svg {...base(p)}><rect x="4" y="4" width="8" height="8" rx="1.2" /><rect x="6.3" y="6.3" width="3.4" height="3.4" rx=".5" /><path d="M6 1.8V4M10 1.8V4M6 12v2.2M10 12v2.2M1.8 6H4M1.8 10H4M12 6h2.2M12 10h2.2" /></svg>
  ),
  processes: (p: P) => (
    <svg {...base(p)}><rect x="2" y="2.5" width="12" height="11" rx="1.5" /><path d="M2 6h12M5 9h2M5 11h4M10.5 9h1" /></svg>
  ),
  search: (p: P) => (
    <svg {...base(p)}><circle cx="7" cy="7" r="4.2" /><path d="M10.2 10.2L13.5 13.5" /></svg>
  ),
  tweak: (p: P) => (
    <svg {...base(p)}><path d="M3 4h6M12 4h1M3 8h1M7 8h6M3 12h6M12 12h1" /><circle cx="10.5" cy="4" r="1.5" /><circle cx="5.5" cy="8" r="1.5" /><circle cx="10.5" cy="12" r="1.5" /></svg>
  ),
  gaming: (p: P) => (
    <svg {...base(p)}><path d="M4.5 4.5h7a3 3 0 013 3v.6a3 3 0 01-5 2.2l-.3-.3H6.8l-.3.3a3 3 0 01-5-2.2v-.6a3 3 0 013-3z" /><path d="M4.8 6.4v2.2M3.7 7.5h2.2" /><path d="M10.6 7.2h.01M11.8 8.3h.01" strokeWidth="2" /></svg>
  ),
  diagnostics: (p: P) => (
    <svg {...base(p)}><path d="M1.8 8.5h2.6l1.6-4 2.4 7 1.6-3h4.2" /></svg>
  ),
  customise: (p: P) => (
    <svg {...base(p)}><rect x="2" y="2.5" width="12" height="11" rx="1.5" /><path d="M2 10.5h12" /><path d="M4.5 12h1.5M7.5 12h1" /><path d="M5 5.5h6M5 7.5h3.5" /></svg>
  ),
  services: (p: P) => (
    <svg {...base(p)}><circle cx="8" cy="8" r="2.2" /><path d="M8 1.8v1.8M8 12.4v1.8M1.8 8h1.8M12.4 8h1.8M3.6 3.6l1.3 1.3M11.1 11.1l1.3 1.3M3.6 12.4l1.3-1.3M11.1 4.9l1.3-1.3" /></svg>
  ),
  bench: (p: P) => (
    <svg {...base(p)}><path d="M2.5 11.5a5.5 5.5 0 1111 0" /><path d="M8 11.5l2.6-3.4" /><circle cx="8" cy="11.5" r=".8" fill="currentColor" /></svg>
  ),
  maintenance: (p: P) => (
    <svg {...base(p)}><rect x="2.5" y="3" width="11" height="10.5" rx="1.5" /><path d="M2.5 6h11M5.5 1.8v2.4M10.5 1.8v2.4" /><path d="M5.8 9.6l1.5 1.4 3-3" /></svg>
  ),
  profiles: (p: P) => (
    <svg {...base(p)}><rect x="2" y="2.5" width="5" height="5" rx="1" /><rect x="9" y="8.5" width="5" height="5" rx="1" /><path d="M7 5h3.5a1 1 0 011 1v2.5M9 11H5.5a1 1 0 01-1-1V7.5" /></svg>
  ),
  storage: (p: P) => (
    <svg {...base(p)}><rect x="2" y="3" width="12" height="10" rx="1.5" /><path d="M2 9.5h12" /><path d="M11 11.3h.01" strokeWidth="2" /></svg>
  ),
  network: (p: P) => (
    <svg {...base(p)}><path d="M1.8 6.2a9 9 0 0112.4 0M3.9 8.6a6 6 0 018.2 0M6 11a3 3 0 014 0" /><circle cx="8" cy="13" r=".6" fill="currentColor" /></svg>
  ),
  startup: (p: P) => (
    <svg {...base(p)}><path d="M8 1.8v5.4" /><path d="M4.4 4a5.6 5.6 0 107.2 0" /></svg>
  ),
  assistant: (p: P) => (
    <svg {...base(p)}><path d="M11.8 9.2A5.2 5.2 0 016.8 2a5.6 5.6 0 105 7.2z" /><path d="M12 2.2v2M11 3.2h2" /></svg>
  ),
  history: (p: P) => (
    <svg {...base(p)}><path d="M2.4 8a5.6 5.6 0 101.6-3.9" /><path d="M2.2 2.6v2.6h2.6" /><path d="M8 5v3.2l2 1.3" /></svg>
  ),
  settings: (p: P) => (
    <svg {...base(p)}><circle cx="8" cy="8" r="2" /><path d="M8 1.6v1.6M8 12.8v1.6M14.4 8h-1.6M3.2 8H1.6M12.5 3.5l-1.1 1.1M4.6 11.4l-1.1 1.1M12.5 12.5l-1.1-1.1M4.6 4.6L3.5 3.5" /></svg>
  ),
  shield: (p: P) => (
    <svg {...base(p)}><path d="M8 1.8l5 1.9v4c0 3.2-2.2 5.4-5 6.5-2.8-1.1-5-3.3-5-6.5v-4z" /></svg>
  ),
  check: (p: P) => (
    <svg {...base(p)}><path d="M3.5 8.4l2.9 2.9 6-6.4" /></svg>
  ),
  x: (p: P) => (
    <svg {...base(p)}><path d="M4 4l8 8M12 4l-8 8" /></svg>
  ),
  arrow: (p: P) => (
    <svg {...base(p)}><path d="M3 8h10M9 4l4 4-4 4" /></svg>
  ),
  send: (p: P) => (
    <svg {...base(p)}><path d="M2.5 8l11-5.5-3.6 11-2.4-4.3z" /><path d="M7.5 9.2l6-6.7" /></svg>
  ),
  stop: (p: P) => (
    <svg {...base(p)}><rect x="4" y="4" width="8" height="8" rx="1.5" /></svg>
  ),
  refresh: (p: P) => (
    <svg {...base(p)}><path d="M13.4 8A5.4 5.4 0 113.8 4.6" /><path d="M2.8 2.4v2.8h2.8" /></svg>
  ),
  undo: (p: P) => (
    <svg {...base(p)}><path d="M5 3.5L2.5 6 5 8.5" /><path d="M2.5 6h7a4 4 0 010 8H7" /></svg>
  ),
  plus: (p: P) => (
    <svg {...base(p)}><path d="M8 3v10M3 8h10" /></svg>
  ),
  library: (p: P) => (
    <svg {...base(p)}><rect x="2" y="2" width="5" height="5" rx="1.2" /><rect x="9" y="2" width="5" height="5" rx="1.2" /><rect x="2" y="9" width="5" height="5" rx="1.2" /><rect x="9" y="9" width="5" height="5" rx="1.2" /></svg>
  ),
  music: (p: P) => (
    <svg {...base(p)}><path d="M6 12.2V3.6l7-1.6v8.6" /><circle cx="4.3" cy="12.2" r="1.8" /><circle cx="11.3" cy="10.6" r="1.8" /></svg>
  ),
  workspaces: (p: P) => (
    <svg {...base(p)}><rect x="1.8" y="4.2" width="9.5" height="8" rx="1.3" /><path d="M4.6 4.2V3a1 1 0 011-1h7.6a1 1 0 011 1v6.6a1 1 0 01-1 1h-1.9" /></svg>
  ),
  studio: (p: P) => (
    <svg {...base(p)}><path d="M8 1.8a6.2 6.2 0 100 12.4c.9 0 1.3-.6 1.1-1.3l-.3-.9c-.3-.9.4-1.7 1.3-1.7h1.6a2.5 2.5 0 002.5-2.5A6.2 6.2 0 008 1.8z" /><circle cx="5" cy="7" r=".6" fill="currentColor" /><circle cx="7.8" cy="4.8" r=".6" fill="currentColor" /><circle cx="10.7" cy="6.2" r=".6" fill="currentColor" /></svg>
  ),
  play: (p: P) => (
    <svg {...base(p)}><path d="M5 3.2v9.6l7.6-4.8z" fill="currentColor" /></svg>
  ),
  pause: (p: P) => (
    <svg {...base(p)}><path d="M5 3v10M11 3v10" strokeWidth="2.2" /></svg>
  ),
  next: (p: P) => (
    <svg {...base(p)}><path d="M3.5 3.5v9l6.3-4.5z" fill="currentColor" /><path d="M12.3 3.5v9" strokeWidth="1.8" /></svg>
  ),
  prev: (p: P) => (
    <svg {...base(p)}><path d="M12.5 3.5v9L6.2 8z" fill="currentColor" /><path d="M3.7 3.5v9" strokeWidth="1.8" /></svg>
  ),
  shuffle: (p: P) => (
    <svg {...base(p)}><path d="M2 4.5h2.4c1.5 0 2.4.8 3.2 2.1l.8 1.4c.8 1.3 1.7 2.1 3.2 2.1H14M2 11.5h2.4c1 0 1.7-.3 2.3-1M9.3 5.5c.6-.7 1.3-1 2.3-1H14" /><path d="M12.3 2.8L14 4.5l-1.7 1.7M12.3 8.8L14 10.5l-1.7 1.7" /></svg>
  ),
  repeat: (p: P) => (
    <svg {...base(p)}><path d="M3 7.2V6a2 2 0 012-2h7.5M13 8.8V10a2 2 0 01-2 2H3.5" /><path d="M10.8 2.3l1.7 1.7-1.7 1.7M5.2 13.7L3.5 12l1.7-1.7" /></svg>
  ),
  volume: (p: P) => (
    <svg {...base(p)}><path d="M2.5 6h2.3L8 3.4v9.2L4.8 10H2.5z" /><path d="M10.5 5.6a3.4 3.4 0 010 4.8M12.3 3.8a6 6 0 010 8.4" /></svg>
  ),
  queue: (p: P) => (
    <svg {...base(p)}><path d="M2 4h9M2 8h9M2 12h5" /><path d="M11 10.5v3l2.6-1.5z" fill="currentColor" /></svg>
  ),
  moon: (p: P) => (
    <svg {...base(p)}><path d="M13.4 9.6A5.6 5.6 0 016.4 2.6a5.6 5.6 0 107 7z" /></svg>
  ),
  smart: (p: P) => (
    <svg {...base(p)}><path d="M8 1.8l1.3 3.4 3.4 1.3-3.4 1.3L8 11.2 6.7 7.8 3.3 6.5l3.4-1.3z" /><path d="M12.6 10.4l.6 1.5 1.5.6-1.5.6-.6 1.5-.6-1.5-1.5-.6 1.5-.6z" /></svg>
  ),
  heart: (p: P) => (
    <svg {...base(p)}><path d="M8 13.2S2.3 9.9 2.3 6a3 3 0 015.7-1.3A3 3 0 0113.7 6c0 3.9-5.7 7.2-5.7 7.2z" /></svg>
  ),
  pin: (p: P) => (
    <svg {...base(p)}><path d="M9.8 2.2l4 4-2.1.7-2.4 2.4.3 2.8-1 1-2.4-2.4-3.2 3.2M5.2 8.5L2.8 6.1l1-1 2.8.3L9 3" /></svg>
  ),
  grid: (p: P) => (
    <svg {...base(p)}><path d="M2 2h5v5H2zM9 2h5v3H9zM9 7h5v7H9zM2 9h5v5H2z" /></svg>
  ),
  lock: (p: P) => (
    <svg {...base(p)}><rect x="3" y="7" width="10" height="7" rx="1.5" /><path d="M5.2 7V5.2a2.8 2.8 0 015.6 0V7" /></svg>
  ),
  bolt: (p: P) => (
    <svg {...base(p)}><path d="M9 1.8L3.6 9h4l-1 5.2L12.4 7h-4z" /></svg>
  ),
  tool: (p: P) => (
    <svg {...base(p)}><path d="M9.8 2.3a3.3 3.3 0 00-3.9 4.5L2 10.7 4.6 13.4l3.9-3.9a3.3 3.3 0 004.5-3.9l-2 2-2.1-.5-.5-2.1z" /></svg>
  ),
  minimize: (p: P) => (
    <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden {...p}><path d="M0 5h10" stroke="currentColor" strokeWidth="1" /></svg>
  ),
  maximize: (p: P) => (
    <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden {...p}><rect x=".5" y=".5" width="9" height="9" fill="none" stroke="currentColor" strokeWidth="1" /></svg>
  ),
  close: (p: P) => (
    <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden {...p}><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" strokeWidth="1" /></svg>
  ),
};
