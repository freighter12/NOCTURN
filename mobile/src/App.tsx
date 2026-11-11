import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

// Build 1: proves the iPhone build pipeline works end to end.
export default function App() {
  const [info, setInfo] = useState<string>("…");

  useEffect(() => {
    invoke<string>("app_info")
      .then(setInfo)
      .catch((e) => setInfo(`Rust bridge error: ${String(e)}`));
  }, []);

  return (
    <main className="hello">
      <svg viewBox="0 0 1024 1024" className="mark" aria-hidden="true">
        <defs>
          <linearGradient id="m" x1="0.2" y1="0.1" x2="0.8" y2="0.95">
            <stop offset="0" stopColor="#F4F5F8" />
            <stop offset="0.55" stopColor="#C9CCD4" />
            <stop offset="1" stopColor="#7E828E" />
          </linearGradient>
          <mask id="c">
            <rect width="1024" height="1024" fill="black" />
            <circle cx="512" cy="512" r="300" fill="white" />
            <circle cx="628" cy="430" r="262" fill="black" />
          </mask>
        </defs>
        <rect width="1024" height="1024" fill="url(#m)" mask="url(#c)" />
        <circle cx="742" cy="300" r="14" fill="#E9EBF0" opacity="0.9" />
      </svg>
      <h1>Nocturn Music</h1>
      <p className="sub">It runs on your iPhone.</p>
      <p className="info">{info}</p>
    </main>
  );
}
