# Releasing NOCTURN

## The update signing key

Installed copies of NOCTURN only accept updates signed with NOCTURN's own key.

- Private key: `%USERPROFILE%\.tauri\nocturn-updater.key` (no password). **Back it up somewhere safe and private** (password manager, encrypted USB). If it's lost, existing installs can't be updated any more. Users would have to reinstall by hand.
- Public key: in `src-tauri/tauri.conf.json` → `plugins.updater.pubkey` (safe to share).
- Never commit the private key. It lives outside the repo on purpose.

## Building a release

1. Bump `version` in `src-tauri/tauri.conf.json` (and `src-tauri/Cargo.toml`, `package.json` to match).
2. `npm run release -- "What's new in this version"`

This builds the NSIS and MSI installers with update signatures (`tauri.release.conf.json` turns on `createUpdaterArtifacts`) and writes `latest.json` next to the NSIS installer in `src-tauri/target/release/bundle/nsis/`. **It doesn't upload anything.** A plain `npm run tauri build` still works without the key.

## Publishing

Installed copies look for `latest.json` at the endpoint in `tauri.conf.json` (`plugins.updater.endpoints`). It's currently `https://github.com/freighter12/NOCTURN/releases/latest/download/latest.json`.

1. Create a GitHub release tagged `v<version>` (it must be the *latest* release, not a pre-release).
2. Upload `NOCTURN_<version>_x64-setup.exe` and `latest.json` from the folder above.

That URL only works for users if the repository (or wherever the endpoint points) is **public**. A private repo returns 404 to installed copies, and the app then reports "No update has been published yet".

## What users see

- A minute after start-up and once a day (Settings › Updates › Check for updates automatically), NOCTURN asks the endpoint for the latest version. It sends nothing about the PC (listed on Settings › Privacy).
- When a newer version exists they get an in-app toast and, if NOCTURN isn't in front, one Windows notification per version.
- Nothing installs until they click **Install … and restart** in Settings › Updates. The download is verified against the public key, then the installer runs in passive mode and reopens NOCTURN. Settings and data are kept (they live in `%APPDATA%\app.nocturn.desktop`).
- Installers are still unsigned for Windows (no code-signing certificate), so SmartScreen may warn on a first install. Updates go through the updater, which checks NOCTURN's own signature.
