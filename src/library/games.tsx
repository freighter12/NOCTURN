// Games in the Library: cover art (batched, cached), the game card, and
// starting a game through its own launcher.

import { useEffect, useState } from "react";
import { I } from "../components/Icons";
import { api, errorMessage, type Game } from "../lib/api";

type Toast = (t: { tone: "good" | "warning" | "critical" | "info"; title: string; detail?: string }) => void;

const covers = new Map<string, string | null>();
const waiting = new Map<string, ((v: string | null) => void)[]>();
let timer: number | undefined;

function flush() {
  timer = undefined;
  const ids = [...waiting.keys()].filter((id) => !covers.has(id));
  for (let i = 0; i < ids.length; i += 40) {
    const batch = ids.slice(i, i + 40);
    api
      .gameCovers(batch)
      .then((m) => batch.forEach((id) => covers.set(id, m[id] ?? null)))
      .catch(() => batch.forEach((id) => covers.set(id, null)))
      .finally(() =>
        batch.forEach((id) => {
          const ws = waiting.get(id) ?? [];
          waiting.delete(id);
          ws.forEach((f) => f(covers.get(id) ?? null));
        }),
      );
  }
}

/** A game's cover (data URL), null when there's none, undefined while loading. */
export function useGameCover(id: string): string | null | undefined {
  const [src, setSrc] = useState<string | null | undefined>(() => covers.get(id));
  useEffect(() => {
    if (covers.has(id)) return setSrc(covers.get(id));
    let live = true;
    const list = waiting.get(id) ?? [];
    list.push((v) => live && setSrc(v));
    waiting.set(id, list);
    timer ??= window.setTimeout(flush, 30);
    return () => {
      live = false;
    };
  }, [id]);
  return src;
}

export async function launchGame(g: Pick<Game, "id" | "name" | "launch_via">, toast: Toast) {
  try {
    await api.launchGame(g.id);
    toast({ tone: "good", title: `Starting ${g.name}`, detail: g.launch_via ? `Opening through ${g.launch_via}.` : undefined });
  } catch (e) {
    toast({ tone: "critical", title: `Couldn't start ${g.name}`, detail: errorMessage(e) });
  }
}

const initials = (name: string) =>
  name
    .split(/\s+/)
    .filter((w) => /[\p{L}\p{N}]/u.test(w))
    .slice(0, 2)
    .map((w) => [...w][0])
    .join("")
    .toUpperCase();

/** Portrait cover, or the game's icon / initials on a styled tile. */
export function GameCover({ game, className = "" }: { game: Pick<Game, "id" | "name">; className?: string }) {
  const src = useGameCover(game.id);
  const [small, setSmall] = useState(false);
  return (
    <div className={`game-cover ${small ? "icon" : ""} ${src ? "" : "empty"} ${className}`} aria-hidden>
      {src ? <img src={src} alt="" draggable={false} onLoad={(e) => setSmall((e.target as HTMLImageElement).naturalWidth < 220)} /> : <span className="game-initials">{initials(game.name)}</span>}
      {(small || !src) && <span className="game-cover-name">{game.name}</span>}
    </div>
  );
}

export function GameCard({ game, toast }: { game: Game; toast: Toast }) {
  const can = !!game.launch_via;
  return (
    <button
      className="game-card"
      onClick={() => (can ? launchGame(game, toast) : toast({ tone: "info", title: `Open ${game.name} from its launcher`, detail: "NOCTURN couldn't find how to start this one." }))}
      title={can ? `Play ${game.name} — opens through ${game.launch_via}` : `${game.name} (${game.launcher})`}
    >
      <div className="game-art">
        <GameCover game={game} />
        {can && (
          <span className="game-play" aria-hidden>
            <I.play />
          </span>
        )}
      </div>
      <span className="game-name truncate">{game.name}</span>
      <span className="game-sub truncate">{game.launcher}</span>
    </button>
  );
}
