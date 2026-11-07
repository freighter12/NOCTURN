// App Library: the user's apps, connected services and websites, plus
// Nocturn's own apps. Everything opens the supported way for that app.

import { useMemo, useState } from "react";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Loading, Note, PageHead, Panel, Seg, Spinner } from "../components/ui";
import { api, errorMessage, library, type LibraryItem, type LibraryService, type StartApp } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { relative } from "../lib/format";
import { AppIcon, NOCTURN_APPS, NocturnIcon, launchItem, whereItOpens } from "../library/tiles";
import { GameCard } from "../library/games";

type Filter = "all" | "games" | "favourites" | "recent" | "nocturn" | "installed" | "connected" | "website";
type AddTab = "installed" | "services" | "website";

function AddPanel({ items, onAdded, onClose }: { items: LibraryItem[]; onAdded: () => void; onClose: () => void }) {
  const { toast } = useApp();
  const [tab, setTab] = useState<AddTab>("installed");
  const start = useLoad(() => library.startApps(), []);
  const services = useLoad(() => library.services(), []);
  const [q, setQ] = useState("");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [site, setSite] = useState({ name: "", url: "https://" });
  const [busy, setBusy] = useState(false);
  const have = new Set(items.map((i) => `${i.kind}|${i.target}`));
  const candidates = (start.data ?? []).filter((a) => !have.has(`installed|${a.app_id}`)).filter((a) => !q || a.name.toLowerCase().includes(q.toLowerCase()));

  async function run(fn: () => Promise<LibraryItem[]>, what: string) {
    setBusy(true);
    try {
      const added = await fn();
      toast({ tone: "good", title: added.length ? `Added ${added.length} ${what}` : "Already in your library" });
      setPicked(new Set());
      onAdded();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't add", detail: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  }

  const addInstalled = (apps: StartApp[]) => run(() => library.add(apps.map((a) => ({ kind: "installed", name: a.name, target: a.app_id, category: a.category }))), apps.length === 1 ? "app" : "apps");
  const addService = (s: LibraryService) => run(() => library.add([{ kind: "connected", name: s.name, target: s.id }]), "service");

  return (
    <Panel className="mt-2" title="Add to your library" actions={<button className="btn ghost sm icon" aria-label="Close" onClick={onClose}><I.close /></button>}>
      <Seg<AddTab> value={tab} onChange={setTab} options={[{ value: "installed", label: "Installed apps" }, { value: "services", label: "Services" }, { value: "website", label: "Website" }]} />
      <div className="mt-2">
        {tab === "installed" && (
          <div className="stack" style={{ gap: 10 }}>
            <div className="row" style={{ gap: 8 }}>
              <input className="input" style={{ flex: 1 }} placeholder={`Search ${start.data?.length ?? ""} apps from your Start menu`} value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search installed apps" />
              <button className="btn" disabled={busy || candidates.length === 0} onClick={() => setPicked(new Set(candidates.map((a) => a.app_id)))}>Select all {q ? "shown" : ""}</button>
              <button className="btn primary" disabled={busy || picked.size === 0} onClick={() => addInstalled((start.data ?? []).filter((a) => picked.has(a.app_id)))}>
                {busy ? <Spinner /> : null} Add {picked.size || ""}
              </button>
            </div>
            {start.loading && !start.data && <Loading label="Reading your Start menu…" />}
            {start.error && <ErrorNote message={start.error} onRetry={start.reload} />}
            <div className="pick-list">
              {candidates.slice(0, 300).map((a) => (
                <label key={a.app_id} className="pick">
                  <input type="checkbox" className="check" checked={picked.has(a.app_id)} onChange={() => setPicked((p) => { const n = new Set(p); n.has(a.app_id) ? n.delete(a.app_id) : n.add(a.app_id); return n; })} />
                  <AppIcon item={{ name: a.name, icon: {} }} appId={a.app_id} size={28} />
                  <span className="truncate">{a.name}</span>
                  <span className="small dim" style={{ marginLeft: "auto" }}>{a.category}</span>
                </label>
              ))}
              {start.data && candidates.length === 0 && <div className="small dim">Everything from your Start menu{q ? " that matches" : ""} is already in your library.</div>}
            </div>
          </div>
        )}
        {tab === "services" && (
          <div className="stack" style={{ gap: 10 }}>
            <div className="small muted">Services open through their official app when it's installed, otherwise their official website. Nocturn doesn't embed or sign in to them for you.</div>
            <div className="grid cols-3" style={{ gap: 10 }}>
              {(services.data ?? []).map((s) => {
                const added = have.has(`connected|${s.id}`);
                return (
                  <div key={s.id} className="service-card">
                    <div className="row" style={{ gap: 10 }}>
                      <AppIcon item={{ name: s.name, icon: {} }} size={34} />
                      <div style={{ minWidth: 0 }}>
                        <b>{s.name}</b>
                        <div className="small dim">{s.description}</div>
                      </div>
                    </div>
                    <div className="row between mt-1">
                      <span className="small muted">{s.has_app ? (s.app_installed ? "App installed" : "Opens the website") : "Website"}</span>
                      <button className="btn sm" disabled={added || busy} onClick={() => addService(s)}>{added ? "Added" : "Add"}</button>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        )}
        {tab === "website" && (
          <div className="row" style={{ gap: 8, alignItems: "flex-end" }}>
            <div className="field" style={{ flex: 1 }}>
              <label htmlFor="lsname">Name</label>
              <input id="lsname" className="input" maxLength={80} placeholder="e.g. School portal" value={site.name} onChange={(e) => setSite({ ...site, name: e.target.value })} />
            </div>
            <div className="field" style={{ flex: 2 }}>
              <label htmlFor="lsurl">Address</label>
              <input id="lsurl" className="input" placeholder="https://…" value={site.url} onChange={(e) => setSite({ ...site, url: e.target.value.trim() })} />
            </div>
            <button className="btn primary" disabled={busy || !/^https:\/\/[^\s]+\.[^\s]+/.test(site.url)} onClick={() => run(() => library.add([{ kind: "website", name: site.name, target: site.url }]), "website").then(() => setSite({ name: "", url: "https://" }))}>Add website</button>
          </div>
        )}
      </div>
    </Panel>
  );
}

function EditPanel({ item, categories, onSaved, onClose }: { item: LibraryItem; categories: string[]; onSaved: () => void; onClose: () => void }) {
  const { toast } = useApp();
  const [name, setName] = useState(item.name);
  const [category, setCategory] = useState(item.category);
  const [glyph, setGlyph] = useState(item.icon.glyph ?? "");
  const [colour, setColour] = useState(item.icon.colour ?? "#3b4a6b");
  // Installed apps can keep the icon Windows has for them.
  const canUseOwn = item.kind === "installed";
  const [own, setOwn] = useState(canUseOwn && !item.icon.glyph && !item.icon.colour);
  async function save() {
    try {
      await library.edit({ id: item.id, name, category, icon: own ? { glyph: null, colour: null } : { glyph: glyph || null, colour } });
      onSaved();
      onClose();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't save", detail: errorMessage(e) });
    }
  }
  return (
    <Panel className="mt-2" title={`Edit ${item.name}`}>
      <div className="row" style={{ gap: 14, alignItems: "flex-end", flexWrap: "wrap" }}>
        <AppIcon item={own ? { name, icon: {}, kind: item.kind, target: item.target } : { name, icon: { glyph, colour } }} size={52} />
        <div className="field" style={{ flex: 2, minWidth: 180 }}>
          <label htmlFor="lename">Name</label>
          <input id="lename" className="input" maxLength={80} value={name} onChange={(e) => setName(e.target.value)} />
        </div>
        <div className="field" style={{ flex: 1, minWidth: 150 }}>
          <label htmlFor="lecat">Category</label>
          <input id="lecat" className="input" list="lecats" maxLength={40} value={category} onChange={(e) => setCategory(e.target.value)} />
          <datalist id="lecats">{categories.map((c) => <option key={c} value={c} />)}</datalist>
        </div>
        {canUseOwn && (
          <div className="field">
            <label>Icon</label>
            <Seg<"own" | "custom"> value={own ? "own" : "custom"} onChange={(v) => setOwn(v === "own")} options={[{ value: "own", label: "App's icon" }, { value: "custom", label: "Custom" }]} />
          </div>
        )}
        {!own && (
          <>
            <div className="field" style={{ width: 110 }}>
              <label htmlFor="leglyph">Icon text</label>
              <input id="leglyph" className="input" maxLength={2} placeholder="Ab or 🎮" value={glyph} onChange={(e) => setGlyph(e.target.value)} />
            </div>
            <div className="field">
              <label htmlFor="lecol">Colour</label>
              <input id="lecol" type="color" value={colour} onChange={(e) => setColour(e.target.value)} />
            </div>
          </>
        )}
        <button className="btn ghost" onClick={onClose}>Cancel</button>
        <button className="btn primary" disabled={!name.trim()} onClick={save}>Save</button>
      </div>
    </Panel>
  );
}

export function Library() {
  const { toast, navigate } = useApp();
  const items = useLoad(() => library.list(), []);
  // Games come from the launchers on this PC (Steam, Epic, Xbox, Battle.net, Riot…), nothing to add by hand.
  const gamesList = useLoad(() => api.games(false), []);
  const [filter, setFilter] = useState<Filter>("all");
  const [category, setCategory] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<LibraryItem | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);

  const all = items.data ?? [];
  const categories = useMemo(() => [...new Set(all.map((i) => i.category).filter(Boolean))].sort(), [all]);
  const match = (name: string) => !q || name.toLowerCase().includes(q.toLowerCase());
  const shown = all
    .filter((i) => match(i.name))
    .filter((i) => !category || i.category === category)
    .filter((i) => {
      switch (filter) {
        case "favourites": return i.favourite;
        case "recent": return !!i.last_opened_at;
        case "installed": case "connected": case "website": return i.kind === filter;
        case "nocturn": case "games": return false;
        default: return true;
      }
    })
    .sort((a, b) => (filter === "recent" ? (b.last_opened_at ?? "").localeCompare(a.last_opened_at ?? "") : 0));
  const pinned = all.filter((i) => i.pinned);
  const nocturn = NOCTURN_APPS.filter((a) => match(a.name));
  const showNocturn = (filter === "all" || filter === "nocturn") && !category;
  const games = (gamesList.data ?? []).filter((g) => !g.not_game && match(g.name));
  const showGames = (filter === "all" || filter === "games") && !category;

  const toggle = async (i: LibraryItem, field: "favourite" | "pinned") => {
    try {
      await library.edit({ id: i.id, [field]: !i[field] });
      items.reload();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't update", detail: errorMessage(e) });
    }
  };

  const tile = (i: LibraryItem) => (
    <div key={i.id} className="app-tile">
      <button className="app-open" onClick={() => launchItem(i, toast, items.reload)} title={whereItOpens(i)}>
        <AppIcon item={i} />
        <span className="app-name truncate">{i.name}</span>
        <span className="small dim truncate">{i.category || "Uncategorised"}</span>
      </button>
      <div className="app-actions">
        <button className={`btn ghost sm icon ${i.favourite ? "on" : ""}`} aria-label={i.favourite ? `Unfavourite ${i.name}` : `Favourite ${i.name}`} title="Favourite" onClick={() => toggle(i, "favourite")}><I.heart /></button>
        <button className={`btn ghost sm icon ${i.pinned ? "on" : ""}`} aria-label={i.pinned ? `Unpin ${i.name}` : `Pin ${i.name}`} title="Pin to the top and to Home" onClick={() => toggle(i, "pinned")}><I.pin /></button>
        <button className="btn ghost sm icon" aria-label={`Edit ${i.name}`} title="Edit" onClick={() => setEditing(i)}><I.tweak /></button>
        {confirmRemove === i.id ? (
          <button className="btn sm" onClick={() => library.remove(i.id).then(() => { setConfirmRemove(null); items.reload(); })}>Remove</button>
        ) : (
          <button className="btn ghost sm icon" aria-label={`Remove ${i.name}`} title="Remove from library (doesn't uninstall)" onClick={() => setConfirmRemove(i.id)}><I.close /></button>
        )}
      </div>
      <div className="small dim app-where">{whereItOpens(i)}{i.last_opened_at ? ` · ${relative(i.last_opened_at)}` : ""}</div>
    </div>
  );

  return (
    <div className="page">
      <PageHead
        eyebrow="Library"
        title="Your games, apps and services"
        desc="Everything you use, in one place. Games are found in your launchers and open through them; apps open in their own windows; services through their official app or website."
        actions={<button className="btn primary" onClick={() => { setAdding((v) => !v); setEditing(null); }}><I.plus /> Add apps</button>}
      />
      {adding && <AddPanel items={all} onAdded={items.reload} onClose={() => setAdding(false)} />}
      {editing && <EditPanel item={editing} categories={categories} onSaved={items.reload} onClose={() => setEditing(null)} />}

      <div className="row between mt-2" style={{ flexWrap: "wrap", gap: 10 }}>
        <Seg<Filter>
          value={filter}
          onChange={setFilter}
          options={[
            { value: "all", label: "All" },
            { value: "games", label: "Games" },
            { value: "favourites", label: "Favourites" },
            { value: "recent", label: "Recent" },
            { value: "nocturn", label: "Nocturn" },
            { value: "installed", label: "Installed" },
            { value: "connected", label: "Services" },
            { value: "website", label: "Websites" },
          ]}
        />
        <input className="input" style={{ width: 240 }} placeholder="Search your library" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search library" />
      </div>
      {categories.length > 0 && (
        <div className="chips mt-1">
          <button className={`chip ${!category ? "on" : ""}`} onClick={() => setCategory(null)}>All categories</button>
          {categories.map((c) => <button key={c} className={`chip ${category === c ? "on" : ""}`} onClick={() => setCategory(category === c ? null : c)}>{c}</button>)}
        </div>
      )}

      {items.error && <div className="mt-2"><ErrorNote message={items.error} onRetry={items.reload} /></div>}
      {pinned.length > 0 && filter === "all" && !q && !category && (
        <>
          <div className="section-title">Pinned</div>
          <div className="app-grid">{pinned.map(tile)}</div>
        </>
      )}
      {showGames && (games.length > 0 || filter === "games") && (
        <>
          <div className="section-title row between">
            <span>Games</span>
            <span className="small dim" style={{ textTransform: "none", letterSpacing: 0, fontWeight: 400 }}>Found in your launchers · opens through each game's own launcher</span>
          </div>
          {gamesList.loading && !gamesList.data && <Loading label="Looking through your game launchers…" />}
          {gamesList.data && games.length === 0 && (
            <Empty title={q ? "No games match" : "No games found"}>NOCTURN looks in Steam, Epic Games, GOG, Battle.net, Riot, EA app, Ubisoft Connect, Rockstar and Xbox / Microsoft Store. Games from anywhere else can be added on the Gaming page.</Empty>
          )}
          <div className="game-grid">{(filter === "games" ? games : games.slice(0, 12)).map((g) => <GameCard key={g.id} game={g} toast={toast} />)}</div>
          {filter === "all" && games.length > 12 && <button className="btn ghost sm mt-1" onClick={() => setFilter("games")}>All {games.length} games</button>}
        </>
      )}
      {showNocturn && nocturn.length > 0 && (
        <>
          <div className="section-title">Nocturn</div>
          <div className="app-grid">
            {nocturn.map((a) => (
              <div key={a.page} className="app-tile">
                <button className="app-open" onClick={() => navigate(a.page)}>
                  <NocturnIcon icon={a.icon} />
                  <span className="app-name truncate">{a.name}</span>
                  <span className="small dim truncate">{a.description}</span>
                </button>
                <div className="small dim app-where">Opens in Nocturn</div>
              </div>
            ))}
          </div>
        </>
      )}
      {filter !== "nocturn" && filter !== "games" && (
        <>
          <div className="section-title">{filter === "recent" ? "Recently opened" : filter === "favourites" ? "Favourites" : "Your library"}</div>
          {items.loading && !items.data && <Loading />}
          {items.data && shown.length === 0 && (
            <Empty title={all.length === 0 ? "Your library is empty" : "Nothing matches"}>
              {all.length === 0 ? "Add apps from your Start menu, services like Discord or Steam, or websites you use." : "Try another filter or search."}
            </Empty>
          )}
          <div className="app-grid">{shown.map(tile)}</div>
        </>
      )}
      <div className="mt-2">
        <Note>Removing something from your library never uninstalls it. Nocturn only opens apps that are in your Start menu, services from its supported list, and https websites you added.</Note>
      </div>
    </div>
  );
}
