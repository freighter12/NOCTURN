import { useEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { fmtBytes, offline, useDownloads } from "../downloads";
import { DAY_PARTS, defaultHome, greetingText, homePrefs, type HomePrefs } from "../greeting";
import { library, useLibrary, type Motion } from "../library";
import { defaultSettings, player, settings, stats, type IntroMode, type LyricsLook, type Refresh, type Settings, type ThemeName } from "../native";
import { useApp, type SettingsSection } from "../state";
import { Icon, PageHeader } from "../ui";
import { HomeHero } from "./Home";
import { buildLabel, updates, useUpdates } from "../update";
import { sync, useSync } from "../sync";

/** The season the Seasons background shows right now (northern hemisphere, like the phone side). */
function seasonNow(): { name: string; id: ThemeName } {
  const m = new Date().getMonth() + 1;
  if (m >= 3 && m <= 5) return { name: "Spring blossom", id: "blossom" };
  if (m >= 6 && m <= 8) return { name: "Summer golden hour", id: "summer" };
  if (m >= 9 && m <= 11) return { name: "Autumn leaves", id: "leaves" };
  return { name: "Christmas lights", id: "christmas" };
}

const THEMES: { id: ThemeName; name: string; about: string }[] = [
  { id: "artwork", name: "Moving Artwork", about: "The song's colours, slowly drifting" },
  { id: "seasons", name: "Seasons", about: `Changes with the season. Now: ${seasonNow().name}` },
  { id: "lunar", name: "Lunar", about: "Night sky with tonight's real moon" },
  { id: "aurora", name: "Aurora", about: "Northern lights rippling over stars" },
  { id: "nebula", name: "Nebula", about: "Swirling clouds of deep space" },
  { id: "synthwave", name: "Synthwave", about: "Neon grid racing to a striped sun" },
  { id: "ocean", name: "Night Ocean", about: "Waves rolling under the moon" },
  { id: "snow", name: "Snowfall", about: "Soft snow drifting down" },
  { id: "fireflies", name: "Fireflies", about: "Little lights glowing in the dark" },
  { id: "rain", name: "Rainy Night", about: "Rain on the glass, city lights below" },
  { id: "meteors", name: "Meteor Shower", about: "Shooting stars across the sky" },
  { id: "galaxy", name: "Galaxy", about: "A spiral galaxy, slowly turning" },
  { id: "lava", name: "Lava Lamp", about: "Warm blobs rising and melting together" },
  { id: "waveform", name: "Waveform", about: "Glowing sound waves flowing by" },
  { id: "city", name: "City Lights", about: "A skyline where windows light up" },
  { id: "bokeh", name: "Bokeh", about: "Soft, blurry lights drifting" },
  { id: "blossom", name: "Blossom", about: "Spring petals in a pink dusk" },
  { id: "summer", name: "Golden Hour", about: "Summer sun setting into the sea" },
  { id: "leaves", name: "Autumn Leaves", about: "Leaves tumbling under a harvest moon" },
  { id: "christmas", name: "Christmas", about: "Fairy lights, snow and pine trees" },
  { id: "black", name: "Pure Black", about: "Sharp on your screen, saves battery" },
  { id: "photo", name: "Your Photo", about: "A picture of your own, blurred" },
];

const SPEEDS = [
  { v: 0.5, label: "Slow" },
  { v: 1, label: "Normal" },
  { v: 1.8, label: "Fast" },
];
const DIMS = [
  { v: 0, label: "Lighter" },
  { v: 1, label: "Normal" },
  { v: 2, label: "Darker" },
];

const SECTIONS: { id: SettingsSection; title: string; icon: string; tint: string }[] = [
  { id: "appearance", title: "Appearance", icon: "palette", tint: "#7c5cff" },
  { id: "home", title: "Home Screen", icon: "greet", tint: "#ff9f0a" },
  { id: "playback", title: "Playback", icon: "play", tint: "#30d158" },
  { id: "lyrics", title: "Lyrics", icon: "mic", tint: "#ff375f" },
  { id: "downloads", title: "Downloads", icon: "download", tint: "#0a84ff" },
  { id: "history", title: "History", icon: "clock", tint: "#64d2ff" },
  { id: "sync", title: "PC Sync", icon: "pc", tint: "#5e5ce6" },
  { id: "about", title: "About", icon: "info", tint: "#8e8e93" },
];

// ---------------------------------------------------------------- shared settings state

let cached: Settings | null = null;

function useSettings(): [Settings | null, (patch: Partial<Settings>) => void] {
  const [s, setS] = useState<Settings | null>(cached);
  useEffect(() => {
    settings
      .get()
      .then((v) => {
        cached = v;
        setS(v);
      })
      .catch(() => {
        cached = cached ?? { ...defaultSettings };
        setS(cached);
      });
  }, []);
  const patch = (p: Partial<Settings>) =>
    setS((cur) => {
      const next = { ...(cur ?? defaultSettings), ...p };
      cached = next;
      return next;
    });
  return [s, patch];
}

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

// ---------------------------------------------------------------- small building blocks

function Group({ title, children, note }: { title?: string; children: ReactNode; note?: string }) {
  return (
    <>
      {title && <h2 className="set-head">{title}</h2>}
      <div className="set-group">{children}</div>
      {note && <p className="set-note">{note}</p>}
    </>
  );
}

function Toggle({ title, sub, on, set }: { title: string; sub?: string; on: boolean; set: (on: boolean) => void }) {
  return (
    <label className="set-row">
      <span>
        <b>{title}</b>
        {sub && <small>{sub}</small>}
      </span>
      <input type="checkbox" className="switch" checked={on} onChange={(e) => set(e.target.checked)} />
    </label>
  );
}

function Segmented<T extends string | number>({ value, options, set }: { value: T; options: { v: T; label: string }[]; set: (v: T) => void }) {
  const i = Math.max(0, options.findIndex((o) => o.v === value));
  return (
    <div className="seg" style={{ ["--n" as string]: options.length, ["--i" as string]: i }}>
      <span className="seg-thumb" />
      {options.map((o) => (
        <button key={String(o.v)} className={o.v === value ? "on" : ""} onClick={() => set(o.v)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

function TextRow({ label, value, placeholder, set, max = 40 }: { label: string; value: string; placeholder: string; set: (v: string) => void; max?: number }) {
  return (
    <label className="set-row set-text">
      <span className="set-label">{label}</span>
      <input
        type="text"
        value={value}
        placeholder={placeholder}
        maxLength={max}
        autoCapitalize="sentences"
        autoComplete="off"
        enterKeyHint="done"
        onChange={(e) => set(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
      />
    </label>
  );
}

function NavRow({ icon, tint, title, detail, onClick }: { icon: string; tint: string; title: string; detail?: string; onClick: () => void }) {
  return (
    <button className="set-row set-nav" onClick={onClick}>
      <span className="set-icon" style={{ background: tint }}>
        <Icon name={icon} size={18} />
      </span>
      <span>
        <b>{title}</b>
        {detail && <small>{detail}</small>}
      </span>
      <Icon name="back" size={18} className="chev" />
    </button>
  );
}

// ---------------------------------------------------------------- the pages

export default function SettingsPage({ section }: { section?: SettingsSection }) {
  const app = useApp();
  const title = section ? (SECTIONS.find((x) => x.id === section)?.title ?? "Settings") : "Settings";
  let body: ReactNode;
  if (section === "appearance") body = <Appearance />;
  else if (section === "home") body = <HomeScreen />;
  else if (section === "playback") body = <Playback />;
  else if (section === "lyrics") body = <Lyrics />;
  else if (section === "downloads") body = <Downloads />;
  else if (section === "history") body = <History />;
  else if (section === "sync") body = <PCSync />;
  else if (section === "about") body = <About />;
  else body = <Root />;
  return (
    <div className="settings">
      <PageHeader title={title} onBack={app.back} />
      {body}
      <div className="bottom-space" />
    </div>
  );
}

function Root() {
  const app = useApp();
  const lib = useLibrary();
  const dl = useDownloads();
  const [s] = useSettings();
  const theme = THEMES.find((t) => t.id === s?.theme)?.name ?? "";
  const detail: Record<SettingsSection, string> = {
    appearance: s ? `${theme}${s.still ? " · Still" : ""}` : "",
    home: `“${greetingText(homePrefs(lib))}”`,
    playback: [app.player.autoplay ? "Autoplay on" : "Autoplay off", app.player.aiShuffle ? "AI Shuffle on" : "", s?.crossfade ? `Crossfade ${s.crossfade}s` : ""].filter(Boolean).join(" · "),
    lyrics: s ? [LYRIC_COLORS.find((c) => c.v === s.lyrics?.color)?.label ?? "White", LYRIC_STYLES.find((x) => x.v === s.lyrics?.style)?.label ?? "Fill"].join(" · ") : "",
    downloads: dl.tracks.length ? `${dl.tracks.length} songs · ${fmtBytes(dl.bytes)}` : "None yet",
    history: `${lib.recent.length} recently played`,
    sync: lib.sync ? `Paired with ${lib.sync.pairing.name}` : "Not paired",
    about: "Version and resets",
  };
  const go = (id: SettingsSection) => app.push({ kind: "settings", section: id });
  const groups: SettingsSection[][] = [["appearance", "home"], ["playback", "lyrics"], ["sync"], ["downloads", "history"], ["about"]];
  return (
    <>
      {groups.map((g, gi) => (
        <div key={gi} className="set-block">
          <div className="set-group">
            {g.map((id) => {
              const sec = SECTIONS.find((x) => x.id === id)!;
              return <NavRow key={id} icon={sec.icon} tint={sec.tint} title={sec.title} detail={detail[id]} onClick={() => go(id)} />;
            })}
          </div>
        </div>
      ))}
    </>
  );
}

function Appearance() {
  const app = useApp();
  const lib = useLibrary();
  const [s, patch] = useSettings();
  const picker = useRef<HTMLInputElement>(null);

  const choose = (id: ThemeName) => {
    if (!s) return;
    if (id === "photo" && !s.photo) {
      picker.current?.click();
      return;
    }
    patch({ theme: id });
    settings.setTheme(id).catch((e) => app.toast(String(e)));
  };

  const picked = async (file?: File) => {
    if (!file || !s) return;
    try {
      const b64 = await shrink(file);
      if (!b64) throw new Error("empty");
      patch({ theme: "photo", photo: b64 });
      await settings.setTheme("photo", b64);
    } catch {
      app.toast("That picture couldn't be used");
    }
  };

  const look = (p: { speed?: number; dim?: number; still?: boolean; intro?: IntroMode; refresh?: Refresh; pulse?: boolean }) => {
    patch(p);
    settings.setLook(p).catch((e) => app.toast(String(e)));
  };

  return (
    <>
      <h2 className="set-head">Background</h2>
      <div className={`theme-grid ${s?.still ? "still" : ""}`} style={{ ["--th-speed" as string]: s ? 1 / (s.still ? 1 : s.speed) : 1 }}>
        {THEMES.map((t) => {
          const on = s?.theme === t.id;
          return (
            <button key={t.id} className={`theme-card ${on ? "on" : ""}`} onClick={() => choose(t.id)} aria-pressed={on}>
              <span className={`theme-preview th-${t.id}`}>
                {t.id === "lunar" && <i className="th-moon" />}
                {t.id === "synthwave" && <i className="th-sun" />}
                {t.id === "ocean" && <i className="th-wave" />}
                {t.id === "seasons" && <span className={`th-season th-${seasonNow().id}`} />}
                {t.id === "seasons" && <span className="th-badge">Special</span>}
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

      <Group title="Motion" note="How fast the background moves. Still Background freezes it, which saves a little battery.">
        <div className="set-row">
          <Segmented value={SPEEDS.find((x) => x.v === s?.speed)?.v ?? 1} options={SPEEDS} set={(v) => look({ speed: v, still: false })} />
        </div>
        <Toggle title="Still Background" on={s?.still ?? false} set={(on) => look({ still: on })} />
        <Toggle title="Beat Pulse" sub="The background glows with the beat of the song. Starts with the next song." on={s?.pulse ?? true} set={(on) => look({ pulse: on })} />
      </Group>

      <Group
        title="Smoothness"
        note={
          (s?.maxFps ?? 60) >= 120
            ? "120 Hz draws every frame your screen can show: the smoothest backgrounds, but it uses more battery. Battery Saver draws half as many. Scrolling and lyrics are always as smooth as your phone allows."
            : "This screen tops out at 60 Hz, so 120 Hz needs an iPhone Pro (13 Pro or newer). Battery Saver draws the background half as often."
        }
      >
        <div className="set-row">
          <Segmented<Refresh>
            value={s?.refresh ?? "auto"}
            options={[
              { v: "auto", label: "Auto" },
              { v: "120", label: (s?.maxFps ?? 60) >= 120 ? "120 Hz" : "Max" },
              { v: "saver", label: "Battery Saver" },
            ]}
            set={(v) => look({ refresh: v })}
          />
        </div>
      </Group>

      <Group title="Darkness Behind Text" note="Darker makes words easier to read over bright backgrounds.">
        <div className="set-row">
          <Segmented value={s?.dim ?? 1} options={DIMS} set={(v) => look({ dim: v })} />
        </div>
      </Group>

      <Group title="Start-up Animation" note="The moon and Nocturn name when the app opens. Tap the screen to skip it.">
        <div className="set-row">
          <Segmented
            value={s?.intro ?? "full"}
            options={[
              { v: "full" as IntroMode, label: "Full" },
              { v: "short" as IntroMode, label: "Quick" },
              { v: "off" as IntroMode, label: "Off" },
            ]}
            set={(v) => look({ intro: v })}
          />
        </div>
      </Group>

      <Group title="Page Animations" note="How pages slide and fade when you move around the app.">
        <div className="set-row">
          <Segmented
            value={lib.prefs?.motion ?? "full"}
            options={[
              { v: "full" as Motion, label: "Full" },
              { v: "subtle" as Motion, label: "Subtle" },
              { v: "off" as Motion, label: "Off" },
            ]}
            set={(v) => library.setMotion(v)}
          />
        </div>
      </Group>
    </>
  );
}

function HomeScreen() {
  const lib = useLibrary();
  const p = homePrefs(lib);
  const set = (patch: Partial<HomePrefs>) => library.setHome(patch);
  const changed = JSON.stringify(p) !== JSON.stringify(defaultHome);

  return (
    <>
      <div className="greet-preview">
        <HomeHero prefs={p} preview />
      </div>

      <Group title="Greeting">
        <TextRow label="Your Name" value={p.name} placeholder="Optional" set={(name) => set({ name })} max={24} />
        <div className="set-row">
          <Segmented
            value={p.mode}
            options={[
              { v: "time", label: "Changes With Time" },
              { v: "custom", label: "My Own Message" },
            ]}
            set={(mode) => set({ mode })}
          />
        </div>
        {p.mode === "custom" ? (
          <TextRow label="Message" value={p.custom} placeholder="Welcome back" set={(custom) => set({ custom })} />
        ) : (
          DAY_PARTS.map((d) => <TextRow key={d.id} label={d.label} value={p[d.id]} placeholder={d.usual} set={(v) => set({ [d.id]: v } as Partial<HomePrefs>)} />)
        )}
      </Group>
      {p.mode === "time" && <p className="set-note">Leave a line empty to keep the usual greeting. Morning is 5 am – noon, afternoon until 6 pm, evening until midnight.</p>}

      <Group title="Small Line Above">
        <TextRow label="Text" value={p.eyebrow} placeholder="Nocturn Music" set={(eyebrow) => set({ eyebrow })} max={30} />
      </Group>

      <Group title="Look">
        <Toggle title="Moon" on={p.moon} set={(moon) => set({ moon })} />
        <Toggle title="Twinkling Stars" on={p.stars} set={(stars) => set({ stars })} />
        <Toggle title="Big Greeting" on={p.big} set={(big) => set({ big })} />
      </Group>

      <Group title="Show on Home">
        <Toggle title="Recently Played" on={p.recent} set={(recent) => set({ recent })} />
        <Toggle title="Suggestions" sub="Mixes and songs picked for you" on={p.picks} set={(picks) => set({ picks })} />
      </Group>

      {changed && (
        <div className="set-block">
          <div className="set-group">
            <button className="set-row danger" onClick={() => library.setHome(null)}>
              <span>
                <b>Reset Home Screen</b>
              </span>
            </button>
          </div>
        </div>
      )}
    </>
  );
}

function Playback() {
  const app = useApp();
  const lib = useLibrary();
  const [s, patch] = useSettings();
  return (
    <>
      <Group>
        <Toggle
          title="Autoplay"
          sub="When your songs run out, keep going with similar music."
          on={app.player.autoplay}
          set={(on) => settings.setAutoplay(on).catch((e) => app.toast(String(e)))}
        />
        <Toggle
          title="AI Shuffle"
          sub="Picks what plays next from your taste and learns from songs you skip."
          on={!!app.player.aiShuffle}
          set={(on) => app.setAIShuffle(on)}
        />
      </Group>
      <Group
        title="Crossfade"
        note="Each song fades into the next. Songs from the same album still play straight through, so albums that run together stay seamless."
      >
        <div className="set-row">
          <Segmented
            value={s?.crossfade ?? 0}
            options={[
              { v: 0, label: "Off" },
              { v: 3, label: "3s" },
              { v: 6, label: "6s" },
              { v: 9, label: "9s" },
              { v: 12, label: "12s" },
            ]}
            set={(v) => {
              patch({ crossfade: v });
              settings.setCrossfade(v).catch((e) => app.toast(String(e)));
            }}
          />
        </div>
      </Group>
      <Group note="Turns loud songs down to the same level as the rest, so you're not reaching for the volume between songs.">
        <Toggle
          title="Volume Levelling"
          sub="Every song at a similar loudness."
          on={s?.levelling ?? true}
          set={(on) => {
            patch({ levelling: on });
            settings.setAudio({ level: on }).catch((e) => app.toast(String(e)));
          }}
        />
      </Group>
      <Group
        title="Other Apps"
        note="With Play Alongside on, opening the camera, a video or a game won't stop your music: both play together. While it's on, iPhone hides the music controls on the lock screen. The mini player in the app still works."
      >
        <Toggle
          title="Play Alongside Other Apps"
          sub="Keep the music going when another app makes sound."
          on={s?.mixWithOthers ?? false}
          set={(on) => {
            patch({ mixWithOthers: on });
            settings.setAudio({ mix: on }).catch((e) => app.toast(String(e)));
          }}
        />
        <Toggle
          title="Carry On After Interruptions"
          sub="After a call, a video or the camera is done with the sound, the music starts again by itself."
          on={s?.autoResume ?? true}
          set={(on) => {
            patch({ autoResume: on });
            settings.setAudio({ resume: on }).catch((e) => app.toast(String(e)));
          }}
        />
      </Group>
      <Group title="Feel">
        <Toggle title="Haptics" sub="Small taps you feel when you press things." on={lib.prefs?.haptics ?? true} set={(on) => library.setHaptics(on)} />
      </Group>
    </>
  );
}

const LYRIC_COLORS: { v: LyricsLook["color"]; label: string; css: string }[] = [
  { v: "white", label: "White", css: "#ffffff" },
  { v: "artwork", label: "Artwork", css: "conic-gradient(#ff8a8a, #ffd27a, #8affc1, #8ab8ff, #d08aff, #ff8a8a)" },
  { v: "moon", label: "Moon", css: "#c7dbff" },
  { v: "gold", label: "Gold", css: "#ffd680" },
  { v: "rose", label: "Rose", css: "#ffb3c7" },
  { v: "mint", label: "Mint", css: "#99ffd6" },
  { v: "violet", label: "Violet", css: "#ccb3ff" },
];
const LYRIC_STYLES: { v: LyricsLook["style"]; label: string }[] = [
  { v: "fill", label: "Fill" },
  { v: "glow", label: "Glow" },
  { v: "bounce", label: "Bounce" },
  { v: "rise", label: "Rise" },
];

function Lyrics() {
  const app = useApp();
  const [s, patch] = useSettings();
  const look = s?.lyrics ?? defaultSettings.lyrics;
  const set = (p: Partial<LyricsLook>) => {
    patch({ lyrics: { ...look, ...p } });
    settings.setLyricsLook(p).catch((e) => app.toast(String(e)));
  };
  const color = LYRIC_COLORS.find((c) => c.v === look.color) ?? LYRIC_COLORS[0];
  return (
    <>
      <div className={`lyric-preview ${look.align === "center" ? "center" : ""} st-${look.style}`} style={{ ["--lc" as string]: color.v === "artwork" ? "#ffd27a" : color.css, ["--ls" as string]: look.size }} aria-hidden="true">
        <p className="past">I been running through the night</p>
        <p className="now">
          {"Moonlight on the window glass".split(" ").map((w, i) => (
            <span key={i} style={{ ["--d" as string]: `${i * 0.45}s` }} className={look.words ? "" : "line"}>
              {w}{" "}
            </span>
          ))}
        </p>
        <p className={`next ${look.blur ? "blur" : ""}`}>Every city sleeps but I</p>
      </div>

      <Group title="Colour" note="The colour the words light up in. Artwork picks it from each song's cover.">
        <div className="swatches">
          {LYRIC_COLORS.map((c) => (
            <button key={c.v} className={`swatch ${c.v === look.color ? "on" : ""}`} onClick={() => set({ color: c.v })} aria-label={c.label} aria-pressed={c.v === look.color}>
              <i style={{ background: c.css }} />
              <small>{c.label}</small>
            </button>
          ))}
        </div>
      </Group>

      <Group title="Effect" note="Fill brightens each word as it's sung. Glow adds a halo. Bounce and Rise make the words move.">
        <div className="set-row">
          <Segmented value={look.style} options={LYRIC_STYLES} set={(v) => set({ style: v })} />
        </div>
        <Toggle title="Word by Word" sub="Off lights up the whole line at once." on={look.words} set={(on) => set({ words: on })} />
      </Group>

      <Group title="Text">
        <div className="set-row">
          <Segmented
            value={look.size}
            options={[
              { v: 0.85, label: "Small" },
              { v: 1, label: "Normal" },
              { v: 1.15, label: "Large" },
              { v: 1.3, label: "Huge" },
            ]}
            set={(v) => set({ size: v })}
          />
        </div>
        <div className="set-row">
          <Segmented
            value={look.align}
            options={[
              { v: "left", label: "Left" },
              { v: "center", label: "Centre" },
            ]}
            set={(v) => set({ align: v })}
          />
        </div>
        <Toggle title="Soften Other Lines" sub="Lines that aren't being sung are slightly blurred." on={look.blur} set={(on) => set({ blur: on })} />
      </Group>

      <Group title="Translation" note="Songs in another language show the line in your language underneath, translated on your iPhone by Apple. The first time, iPhone may ask to download that language.">
        <Toggle title="Translate Lyrics" on={look.translate ?? true} set={(on) => set({ translate: on })} />
      </Group>

      <Group
        title="Timing"
        note="Lyrics running early or late on a song? In Now Playing, press and hold the line you're hearing and choose Sync to This Line. Nocturn remembers it for that song. Hold a line and choose Share as Card to make a picture of it for your story."
      >
        <Toggle
          title="Live Lyrics in the Dynamic Island"
          sub="The line being sung shows at the top of the screen and on the lock screen."
          on={s?.liveLyrics ?? true}
          set={(on) => {
            patch({ liveLyrics: on });
            settings.setLiveLyrics(on);
          }}
        />
      </Group>
    </>
  );
}

function Downloads() {
  const app = useApp();
  const dl = useDownloads();
  const [confirm, setConfirm] = useState(false);
  return (
    <Group note="Downloaded songs play without internet, even in Airplane mode.">
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
    </Group>
  );
}

function History() {
  const app = useApp();
  const lib = useLibrary();
  const [confirmStats, setConfirmStats] = useState(false);
  return (
    <Group note="This only clears the lists on this phone. Your liked songs and playlists stay.">
      <button
        className="set-row danger"
        disabled={!lib.recent.length}
        onClick={() => {
          library.clearRecent();
          app.toast("Recently played cleared");
        }}
      >
        <span>
          <b>Clear Recently Played</b>
          <small>{lib.recent.length} songs</small>
        </span>
      </button>
      <button
        className="set-row danger"
        disabled={!lib.searches.length}
        onClick={() => {
          library.clearSearches();
          app.toast("Search history cleared");
        }}
      >
        <span>
          <b>Clear Search History</b>
          <small>{lib.searches.length} searches</small>
        </span>
      </button>
      <button
        className={`set-row danger ${confirmStats ? "confirm" : ""}`}
        onClick={() => {
          if (!confirmStats) return setConfirmStats(true);
          stats.clear().then(() => app.toast("Listening stats cleared")).catch((e) => app.toast(String(e)));
          setConfirmStats(false);
        }}
      >
        <span>
          <b>{confirmStats ? "Tap Again to Clear Your Stats" : "Clear Listening Stats"}</b>
          <small>Starts Nocturn Wrapped from zero</small>
        </span>
      </button>
    </Group>
  );
}

function PCSync() {
  const app = useApp();
  const lib = useLibrary();
  const st = useSync();
  const [pairing, setPairing] = useState(false);
  const s = lib.sync;

  const pair = async () => {
    setPairing(true);
    try {
      const p = await sync.pair();
      player.haptic("success");
      app.toast(`Paired with ${p.name}`);
      await sync.run().then((r) => r && app.toast(syncSummary(r)));
    } catch (e) {
      const msg = String(e);
      if (!/cancel/i.test(msg)) app.toast(msg.replace(/^Error: /, ""));
    } finally {
      setPairing(false);
    }
  };
  const run = () =>
    sync
      .run()
      .then((r) => {
        if (!r) return;
        player.haptic("success");
        app.toast(syncSummary(r));
      })
      .catch(() => {});

  if (!s) {
    return (
      <>
        <div className="sync-hero">
          <span className="sync-icon">
            <Icon name="pc" size={30} />
          </span>
          <b>Your music on your PC and your iPhone</b>
          <p>Playlists and liked songs sync between Nocturn here and NOCTURN on your PC, over your own Wi-Fi. No account, nothing online.</p>
        </div>
        <Group title="How to Pair" note="You only pair once. After that, syncing happens by itself when Nocturn opens (or tap Sync Now).">
          <div className="sync-steps">
            <p><b>1.</b> On your PC, open NOCTURN › Music › Sources and switch on <b>Sync with iPhone</b>. A QR code appears.</p>
            <p><b>2.</b> Make sure this iPhone is on the same Wi-Fi as the PC.</p>
            <p><b>3.</b> Tap <b>Pair with PC</b> and point the camera at the code.</p>
          </div>
        </Group>
        <button className="glass-pill wide sync-cta" disabled={pairing} onClick={pair}>
          {pairing ? "Pairing…" : "Pair with PC"}
        </button>
      </>
    );
  }

  return (
    <>
      <div className="sync-hero">
        <span className="sync-icon on">
          <Icon name="pc" size={30} />
        </span>
        <b>Paired with {s.pairing.name}</b>
        <p>{s.last ? `Last synced ${new Date(s.last.at).toLocaleString(undefined, { weekday: "short", hour: "numeric", minute: "2-digit" })}` : "Not synced yet"}</p>
      </div>
      {st.busy && (
        <div className="import-progress sync-progress">
          <div className="bar">
            <i style={{ width: st.progress?.total ? `${(st.progress.done / st.progress.total) * 100}%` : "10%" }} />
          </div>
          <span>{st.progress?.total ? `Finding your PC's songs… ${st.progress.done} of ${st.progress.total}` : `Talking to ${s.pairing.name}…`}</span>
        </div>
      )}
      {st.error && <p className="import-error sync-error">{st.error.replace(/^Error: /, "")}</p>}
      <button className="glass-pill wide sync-cta" disabled={st.busy} onClick={run}>
        {st.busy ? "Syncing…" : "Sync Now"}
      </button>
      <Group note="When your PC is playing something, opening Nocturn offers to carry on here from the same spot (your PC pauses).">
        <button
          className="set-row"
          disabled={st.busy}
          onClick={async () => {
            try {
              const r = await sync.takeOver();
              if (r?.track?.videoId) {
                app.playList([r.track], 0, { positionMs: r.positionMs });
                app.toast(`Carrying on with ${r.track.title}`);
              } else app.toast("Nothing is playing on your PC");
            } catch (e) {
              app.toast(String(e).replace(/^Error: /, ""));
            }
          }}
        >
          <span>
            <b>Continue from PC</b>
            <small>Play what your PC is playing, from the same spot</small>
          </span>
        </button>
      </Group>
      <Group note="Your PC's playlists show up in Library › Your Playlists, marked PC. Playlists you make or import here go to your PC. Liked songs are added both ways, never removed.">
        <Toggle title="Sync When Nocturn Opens" on={s.auto} set={(on) => library.setSync({ ...s, auto: on })} />
      </Group>
      <Group>
        <button className="set-row" disabled={pairing} onClick={pair}>
          <span>
            <b>Pair Again</b>
            <small>If your PC's address changed or you made a new code</small>
          </span>
        </button>
        <button
          className="set-row danger"
          onClick={() => {
            sync.unpair();
            app.toast("Unpaired");
          }}
        >
          <span>
            <b>Unpair</b>
            <small>Playlists already synced stay on this phone</small>
          </span>
        </button>
      </Group>
    </>
  );
}

function syncSummary(r: { pulled: number; liked: number; pushed: { playlists: number } }): string {
  const parts = [`${r.pulled} from your PC`, `${r.pushed.playlists} to your PC`];
  return `Synced: ${parts.join(", ")}${r.liked ? `, ${r.liked} new liked` : ""}`;
}

function About() {
  const app = useApp();
  const u = useUpdates();
  const [, patch] = useSettings();
  const [version, setVersion] = useState("");
  useEffect(() => {
    invoke<string>("app_info").then(setVersion).catch(() => {});
  }, []);
  return (
    <>
      <div className="about-card">
        <svg viewBox="0 0 1024 1024" className="about-moon" aria-hidden="true">
          <defs>
            <mask id="about-c">
              <rect width="1024" height="1024" fill="black" />
              <circle cx="512" cy="512" r="300" fill="white" />
              <circle cx="628" cy="430" r="262" fill="black" />
            </mask>
          </defs>
          <rect width="1024" height="1024" fill="#e9ebf1" mask="url(#about-c)" />
        </svg>
        <b>Nocturn Music</b>
        <small>{u.current ? `${buildLabel(u.current)} · ${version}` : version || "for iPhone"}</small>
      </div>
      <Group
        title="Updates"
        note={
          u.available
            ? "Your songs, playlists, downloads and settings stay when you update."
            : "Nocturn checks for a new build each time it opens."
        }
      >
        {u.available && u.latest ? (
          <div className="update-card">
            <b>{buildLabel(u.latest.build)} is ready</b>
            <small>You have {buildLabel(u.current)}{u.latest.date ? ` · out since ${new Date(u.latest.date).toLocaleDateString()}` : ""}</small>
            <ol>
              <li>Plug your iPhone into your PC and unlock it.</li>
              <li>Open <b>iloader</b> on the PC.</li>
              <li>Choose <b>Import IPA</b> and pick <b>NocturnMusic.ipa</b> (Claude puts the newest one in your Downloads folder).</li>
              <li>Wait for it to finish, then open Nocturn.</li>
            </ol>
          </div>
        ) : (
          <div className="set-row">
            <span>
              <b>{u.checking ? "Checking…" : u.error ? "Couldn't check" : u.latest ? "You're up to date" : "Not checked yet"}</b>
              <small>{u.error ? "Check your internet connection." : u.latest ? `Newest is ${buildLabel(u.latest.build)}` : ""}</small>
            </span>
          </div>
        )}
        <button className="set-row" disabled={u.checking} onClick={() => updates.check()}>
          <span>
            <b>Check for Updates</b>
          </span>
        </button>
      </Group>
      <Group title="Reset">
        <button
          className="set-row danger"
          onClick={() => {
            const look = { speed: 1, dim: 1, still: false };
            patch({ ...look, theme: "artwork" });
            settings.setLook(look);
            settings.setTheme("artwork");
            app.toast("Look reset");
          }}
        >
          <span>
            <b>Reset Look</b>
            <small>Background, motion and darkness back to normal</small>
          </span>
        </button>
        <button
          className="set-row danger"
          onClick={() => {
            library.setHome(null);
            app.toast("Home screen reset");
          }}
        >
          <span>
            <b>Reset Home Screen</b>
            <small>Greeting and what Home shows</small>
          </span>
        </button>
      </Group>
    </>
  );
}
