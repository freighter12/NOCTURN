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
  tweak: (p: P) => (
    <svg {...base(p)}><path d="M3 4h6M12 4h1M3 8h1M7 8h6M3 12h6M12 12h1" /><circle cx="10.5" cy="4" r="1.5" /><circle cx="5.5" cy="8" r="1.5" /><circle cx="10.5" cy="12" r="1.5" /></svg>
  ),
  gaming: (p: P) => (
    <svg {...base(p)}><path d="M4.5 4.5h7a3 3 0 013 3v.6a3 3 0 01-5 2.2l-.3-.3H6.8l-.3.3a3 3 0 01-5-2.2v-.6a3 3 0 013-3z" /><path d="M4.8 6.4v2.2M3.7 7.5h2.2" /><path d="M10.6 7.2h.01M11.8 8.3h.01" strokeWidth="2" /></svg>
  ),
  diagnostics: (p: P) => (
    <svg {...base(p)}><path d="M1.8 8.5h2.6l1.6-4 2.4 7 1.6-3h4.2" /></svg>
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
