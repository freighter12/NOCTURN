import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Nocturn Music for iPhone. Port 1421 so it never clashes with the desktop app's 1420.
// `@shared` points at the desktop app's music logic (pure TypeScript, no desktop UI).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1421, strictPort: true, host: process.env.TAURI_DEV_HOST || false, watch: { ignored: ["**/src-tauri/**"] } },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  resolve: { alias: { "@shared": new URL("../src/music", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1") } },
  build: { target: "safari18", sourcemap: false },
});
