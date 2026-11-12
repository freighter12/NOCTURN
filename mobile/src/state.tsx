import { createContext, useContext } from "react";
import type { Card, Track } from "./api";
import type { PlayerState } from "./native";

export type TabId = "home" | "search" | "library";

export type Route =
  | { kind: "root" }
  | { kind: "collection"; id: string; seed?: Card }
  | { kind: "artist"; id: string; seed?: Card }
  | { kind: "mine"; id: string }
  | { kind: "liked" }
  | { kind: "recent" }
  | { kind: "saved"; what: "albums" | "playlists" | "artists" }
  | { kind: "downloads" }
  | { kind: "settings"; section?: SettingsSection };

export type SettingsSection = "appearance" | "home" | "playback" | "lyrics" | "downloads" | "history" | "about";

export type AppCtx = {
  tab: TabId;
  route: Route;
  depth: number;
  push(r: Route): void;
  back(): void;
  selectTab(t: TabId): void;
  player: PlayerState;
  /** Songs the app sent to the player, in order (the player may add radio songs after these). */
  queue: Track[];
  playList(tracks: Track[], index?: number, opts?: { shuffle?: boolean }): void;
  playNext(t: Track[]): void;
  addToQueue(t: Track[]): void;
  startRadio(t: Track): void;
  openCard(c: Card): void;
  openActions(t: Track): void;
  openQueue(): void;
  openImport(): void;
  toast(text: string): void;
  /** Turns AI Shuffle on/off and, when on, plans what plays next. */
  setAIShuffle(on: boolean): void;
};

export const AppContext = createContext<AppCtx | null>(null);

export function useApp(): AppCtx {
  const c = useContext(AppContext);
  if (!c) throw new Error("useApp outside provider");
  return c;
}
