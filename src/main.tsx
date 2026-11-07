import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { startTelemetry } from "./lib/telemetry";
import { watchErrors } from "./lib/errors";
import "./styles/global.css";

// Block the WebView's default context menu and browser shortcuts that make no
// sense in a desktop app (reload, find, print), but keep text-field editing.
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest("input, textarea, .selectable")) e.preventDefault();
});
window.addEventListener("keydown", (e) => {
  if ((e.ctrlKey && ["r", "p", "f", "g"].includes(e.key.toLowerCase())) || e.key === "F5") e.preventDefault();
});

watchErrors();
startTelemetry();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
