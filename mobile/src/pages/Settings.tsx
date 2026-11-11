import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes, offline, useDownloads } from "../downloads";
import { settings, type Settings, type ThemeName } from "../native";
import { useApp } from "../state";
import { Icon, PageHeader } from "../ui";

const THEMES: { id: ThemeName; name: string; about: string }[] = [
  { id: "artwork", name: "Moving Artwork", about: "The song's colours, slowly drifting" },
  { id: "lunar", name: "Lunar", about: "Night sky with tonight's real moon" },
  { id: "black", name: "Pure Black", about: "Sharp on your screen, saves battery" },
  { id: "photo", name: "Your Photo", about: "A picture of your own, blurred" },
];

/** Shrinks a picked photo to a ~1400 px JPEG (base64, no data: prefix). */
async function shrink(file: File): Promise<string> {
  const url = URL.createObjectURL(file);
  try {
    const img = new Image();
    img.src = url;
    await img.decode();
    const scale = Math.min(1, 1400 / Math.max(img.naturalWidth, img.naturalHeight));
    const c = document.createElement("canvas");
    c.width = Math.round(img.naturalWidth * scale);
    c.height = Math.round(img.naturalHeight * scale);
    c.getContext("2d")!.drawImage(img, 0, 0, c.width, c.height);
    return c.toDataURL("image/jpeg", 0.82).split(",")[1] ?? "";
  } finally {
    URL.revokeObjectURL(url);
  }
}

export default function SettingsPage() {
  const app = useApp();
  const dl = useDownloads();
  const [s, setS] = useState<Settings | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [version, setVersion] = useState("");
  const picker = useRef<HTMLInputElement>(null);

  useEffect(() => {
    settings.get().then(setS).catch(() => setS({ theme: "artwork", photo: null, liveLyrics: true }));
    invoke<string>("app_info").then(setVersion).catch(() => {});
  }, []);

  const choose = (id: ThemeName) => {
    if (!s) return;
    if (id === "photo" && !s.photo) {
      picker.current?.click();
      return;
    }
    setS({ ...s, theme: id });
    settings.setTheme(id).catch((e) => app.toast(String(e)));
  };

  const picked = async (file?: File) => {
    if (!file || !s) return;
    try {
      const b64 = await shrink(file);
      if (!b64) throw new Error("empty");
      setS({ ...s, theme: "photo", photo: b64 });
      await settings.setTheme("photo", b64);
    } catch {
      app.toast("That picture couldn't be used");
    }
  };

  return (
    <div className="settings">
      <PageHeader title="Settings" onBack={app.back} />

      <h2 className="set-head">Background</h2>
      <div className="theme-grid">
        {THEMES.map((t) => {
          const on = s?.theme === t.id;
          return (
            <button key={t.id} className={`theme-card ${on ? "on" : ""}`} onClick={() => choose(t.id)} aria-pressed={on}>
              <span className={`theme-preview th-${t.id}`}>
                {t.id === "lunar" && <i className="th-moon" />}
                {t.id === "photo" && s?.photo && <img src={`data:image/jpeg;base64,${s.photo}`} alt="" />}
                {t.id === "photo" && !s?.photo && <span className="th-add">+</span>}
                {on && (
                  <span className="th-check">
                    <Icon name="check" size={16} />
                  </span>
                )}
              </span>
              <b>{t.name}</b>
              <small>{t.about}</small>
            </button>
          );
        })}
      </div>
      {s?.photo && (
        <button className="link set-link" onClick={() => picker.current?.click()}>
          Choose a Different Photo
        </button>
      )}
      <input ref={picker} type="file" accept="image/*" hidden onChange={(e) => picked(e.target.files?.[0] ?? undefined).finally(() => (e.target.value = ""))} />

      <h2 className="set-head">Lyrics</h2>
      <div className="set-group">
        <label className="set-row">
          <span>
            <b>Live Lyrics in the Dynamic Island</b>
            <small>The line being sung, at the top of the screen and on the lock screen.</small>
          </span>
          <input
            type="checkbox"
            className="switch"
            checked={s?.liveLyrics ?? true}
            onChange={(e) => {
              if (!s) return;
              setS({ ...s, liveLyrics: e.target.checked });
              settings.setLiveLyrics(e.target.checked);
            }}
          />
        </label>
      </div>

      <h2 className="set-head">Downloads</h2>
      <div className="set-group">
        <button className="set-row" onClick={() => app.push({ kind: "downloads" })}>
          <span>
            <b>Downloaded Songs</b>
            <small>{dl.tracks.length ? `${dl.tracks.length} songs · ${fmtBytes(dl.bytes)}` : "None yet"}</small>
          </span>
          <Icon name="back" size={18} className="chev" />
        </button>
        {dl.tracks.length > 0 &&
          (confirm ? (
            <div className="set-row">
              <span>
                <b>Remove all downloads?</b>
              </span>
              <span className="set-actions">
                <button
                  className="pill danger"
                  onClick={() => {
                    offline.removeAll();
                    setConfirm(false);
                    app.toast("Downloads removed");
                  }}
                >
                  Remove
                </button>
                <button className="pill" onClick={() => setConfirm(false)}>
                  Cancel
                </button>
              </span>
            </div>
          ) : (
            <button className="set-row danger" onClick={() => setConfirm(true)}>
              <span>
                <b>Remove All Downloads</b>
              </span>
            </button>
          ))}
      </div>

      <p className="set-about">Nocturn Music {version}</p>
      <div className="bottom-space" />
    </div>
  );
}
