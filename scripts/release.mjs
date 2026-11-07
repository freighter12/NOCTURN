// Builds signed installers and the updater manifest (latest.json).
// It does NOT publish anything: upload the files it lists to a GitHub release yourself.
//
//   npm run release -- "What's new in this version"
//
// Signing key: %USERPROFILE%\.tauri\nocturn-updater.key (or TAURI_SIGNING_PRIVATE_KEY).
// Keep that file safe — installed copies only accept updates signed with it.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1")), "..");
const conf = JSON.parse(fs.readFileSync(path.join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const version = conf.version;
const endpoint = conf.plugins?.updater?.endpoints?.[0] ?? "";
const notes = process.argv.slice(2).join(" ").trim() || `NOCTURN ${version}`;

const keyPath = path.join(os.homedir(), ".tauri", "nocturn-updater.key");
const env = { ...process.env };
if (!env.TAURI_SIGNING_PRIVATE_KEY) {
  if (!fs.existsSync(keyPath)) {
    console.error(`No signing key. Expected ${keyPath} or TAURI_SIGNING_PRIVATE_KEY.`);
    process.exit(1);
  }
  env.TAURI_SIGNING_PRIVATE_KEY = keyPath;
}
env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ??= "";

console.log(`Building NOCTURN ${version} with updater artifacts…`);
const build = spawnSync(`npx tauri build --config "${path.join("src-tauri", "tauri.release.conf.json")}"`, { cwd: root, env, stdio: "inherit", shell: true });
if (build.status !== 0) process.exit(build.status ?? 1);

const nsisDir = path.join(root, "src-tauri", "target", "release", "bundle", "nsis");
const setup = fs.readdirSync(nsisDir).find((f) => f.endsWith("-setup.exe") && f.includes(`_${version}_`));
if (!setup || !fs.existsSync(path.join(nsisDir, `${setup}.sig`))) {
  console.error(`Couldn't find the signed installer for ${version} in ${nsisDir}.`);
  process.exit(1);
}
const signature = fs.readFileSync(path.join(nsisDir, `${setup}.sig`), "utf8").trim();

// Releases are expected under the same GitHub repo as the endpoint, tagged v<version>.
const m = endpoint.match(/^https:\/\/github\.com\/([^/]+\/[^/]+)\/releases\//);
const url = m ? `https://github.com/${m[1]}/releases/download/v${version}/${setup}` : `REPLACE_WITH_DOWNLOAD_URL/${setup}`;
const manifest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: { "windows-x86_64": { signature, url } },
};
fs.writeFileSync(path.join(nsisDir, "latest.json"), JSON.stringify(manifest, null, 2) + "\n");

console.log(`
Done. To publish (nothing has been uploaded):
  1. Create a GitHub release tagged v${version}${m ? ` in ${m[1]}` : ""}.
  2. Upload these two files to it:
       ${path.join(nsisDir, setup)}
       ${path.join(nsisDir, "latest.json")}
  Installed copies find it through ${endpoint || "(no endpoint configured)"}.
`);
