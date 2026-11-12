// Nocturn Wrapped: how much you've listened, when, and to what.
import { useEffect, useMemo, useState } from "react";
import { stats as nativeStats } from "../native";
import { fmtListen, hourLabel, summarize, type Period, type StatsFile } from "../stats";
import { useApp } from "../state";
import { Art, PageHeader } from "../ui";

const PERIODS: { v: Period; label: string; title: string }[] = [
  { v: "week", label: "Week", title: "the last 7 days" },
  { v: "month", label: "Month", title: "the last 30 days" },
  { v: "year", label: "Year", title: String(new Date().getFullYear()) },
  { v: "all", label: "All Time", title: "all time" },
];

export default function StatsPage() {
  const app = useApp();
  const [data, setData] = useState<StatsFile | null>(null);
  const [period, setPeriod] = useState<Period>("week");
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    nativeStats
      .get()
      .then(setData)
      .catch(() => setFailed(true));
  }, []);

  const s = useMemo(() => (data ? summarize(data, period) : null), [data, period]);
  const p = PERIODS.find((x) => x.v === period)!;
  const max = s ? Math.max(1, ...s.bars.map((b) => b.ms)) : 1;
  const peak = s?.peakHour != null ? hourLabel(s.peakHour) : null;
  const minutes = s ? Math.round(s.ms / 60_000) : 0;

  return (
    <div className="stats">
      <PageHeader title="Nocturn Wrapped" onBack={app.back} />
      <div className="stats-periods">
        {PERIODS.map((x) => (
          <button key={x.v} className={x.v === period ? "on" : ""} onClick={() => setPeriod(x.v)}>
            {x.label}
          </button>
        ))}
      </div>

      {failed && <p className="empty">Your stats couldn't be loaded.</p>}
      {s && s.ms < 60_000 && (
        <div className="empty-state">
          <h3>Nothing yet for {p.title}</h3>
          <p>Play some music and your listening time, top songs and top artists show up here.</p>
        </div>
      )}

      {s && s.ms >= 60_000 && (
        <>
          <section className="wrap-hero">
            <small>You listened for</small>
            <b>{fmtListen(s.ms)}</b>
            <span>
              {minutes.toLocaleString()} minutes in {p.title}
              {s.change != null && (
                <em className={s.change >= 0 ? "up" : "down"}>
                  {s.change >= 0 ? "▲" : "▼"} {Math.abs(Math.round(s.change * 100))}%
                </em>
              )}
            </span>
          </section>

          <section className="wrap-card">
            <div className="wrap-bars" style={{ ["--n" as string]: s.bars.length }}>
              {s.bars.map((b, i) => (
                <div key={i} className={`wrap-bar ${b.today ? "today" : ""}`} title={`${b.label}: ${fmtListen(b.ms)}`}>
                  <i style={{ height: `${Math.max(b.ms ? 4 : 0, (b.ms / max) * 100)}%`, animationDelay: `${i * 0.02}s` }} />
                  {(s.bars.length <= 12 || i % 5 === 0) && <small>{b.label}</small>}
                </div>
              ))}
            </div>
          </section>

          <div className="wrap-tiles">
            <div className="wrap-tile">
              <b>{s.plays.toLocaleString()}</b>
              <small>songs played</small>
            </div>
            <div className="wrap-tile">
              <b>{s.songs.toLocaleString()}</b>
              <small>different songs</small>
            </div>
            <div className="wrap-tile">
              <b>{s.artists.toLocaleString()}</b>
              <small>artists</small>
            </div>
            <div className="wrap-tile">
              <b>{s.streak}</b>
              <small>{s.streak === 1 ? "day streak" : "days in a row"}</small>
            </div>
          </div>

          {peak && (
            <section className="wrap-card wrap-peak">
              <span className="wrap-peak-icon">{s.peakHour! >= 20 || s.peakHour! < 6 ? "🌙" : "☀️"}</span>
              <div>
                <b>{peak.kind}</b>
                <small>
                  You listen most around {peak.time}
                  {s.bestDay ? `. Your biggest day: ${s.bestDay.date.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "short" })}, ${fmtListen(s.bestDay.ms)}.` : "."}
                </small>
              </div>
            </section>
          )}

          {s.topArtists.length > 0 && (
            <section className="wrap-section">
              <h2>Top Artists</h2>
              {s.topArtists.slice(0, 5).map((a, i) => (
                <div key={a.name} className="wrap-row">
                  <span className="wrap-rank">{i + 1}</span>
                  <Art src={a.artwork} size={46} round />
                  <span className="wrap-text">
                    <b>{a.name}</b>
                    <small>{fmtListen(a.ms)}</small>
                  </span>
                </div>
              ))}
            </section>
          )}

          {s.topSongs.length > 0 && (
            <section className="wrap-section">
              <h2>Top Songs</h2>
              {s.topSongs.slice(0, 10).map((t, i) => (
                <button
                  key={t.track.videoId}
                  className="wrap-row"
                  onClick={() => app.playList(s.topSongs.map((x) => x.track), i)}
                >
                  <span className="wrap-rank">{i + 1}</span>
                  <Art src={t.track.artwork} size={46} radius={7} />
                  <span className="wrap-text">
                    <b>{t.track.title}</b>
                    <small>
                      {t.track.artist} · {t.plays} {t.plays === 1 ? "play" : "plays"}
                    </small>
                  </span>
                </button>
              ))}
            </section>
          )}
          <p className="wrap-note">Counted on this iPhone only. Nothing leaves your phone.</p>
        </>
      )}
      <div className="bottom-space" />
    </div>
  );
}
