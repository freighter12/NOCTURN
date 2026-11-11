import { useEffect, useRef, useState } from "react";
import { api, errorText, type SearchResults } from "../api";
import { library, useLibrary } from "../library";
import { useApp } from "../state";
import { Art, ErrorBox, Icon, Loading, Scroller, Shelf, Tile, TrackRow } from "../ui";

// Remember the last search while switching tabs.
let lastQuery = "";
let lastResults: SearchResults | null = null;

export default function Search() {
  const app = useApp();
  const lib = useLibrary();
  const [q, setQ] = useState(lastQuery);
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [results, setResults] = useState<SearchResults | null>(lastResults);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [focused, setFocused] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  // suggestions while typing
  useEffect(() => {
    if (!focused || !q.trim()) {
      setSuggestions([]);
      return;
    }
    const t = setTimeout(() => api.suggest(q).then(setSuggestions).catch(() => {}), 180);
    return () => clearTimeout(t);
  }, [q, focused]);

  const run = (query: string) => {
    const s = query.trim();
    if (!s) return;
    setQ(s);
    lastQuery = s;
    input.current?.blur();
    setFocused(false);
    setBusy(true);
    setError(null);
    library.searched(s);
    api
      .search(s)
      .then((r) => {
        setResults(r);
        lastResults = r;
      })
      .catch((e) => setError(errorText(e)))
      .finally(() => setBusy(false));
  };

  const clear = () => {
    setQ("");
    setResults(null);
    lastQuery = "";
    lastResults = null;
    input.current?.focus();
  };

  const showSuggest = focused && q.trim().length > 0;
  const top = results?.top;

  return (
    <div className="search">
      <h1 className="large-title">Search</h1>
      <form
        className="searchbar"
        onSubmit={(e) => {
          e.preventDefault();
          run(q);
        }}
      >
        <Icon name="search" size={18} />
        <input
          ref={input}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onFocus={() => setFocused(true)}
          onBlur={() => setTimeout(() => setFocused(false), 150)}
          placeholder="Artists, songs, albums"
          enterKeyHint="search"
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck={false}
        />
        {q && (
          <button type="button" className="clear" onClick={clear} aria-label="Clear">
            <Icon name="close" size={14} />
          </button>
        )}
      </form>

      {showSuggest && (
        <ul className="suggest">
          {suggestions.map((s) => (
            <li key={s}>
              <button onMouseDown={(e) => e.preventDefault()} onClick={() => run(s)}>
                <Icon name="search" size={16} />
                {s}
              </button>
            </li>
          ))}
        </ul>
      )}

      {!showSuggest && !results && !busy && lib.searches.length > 0 && (
        <section className="recents">
          <header className="shelf-head">
            <h2>Recently Searched</h2>
            <button className="link" onClick={() => library.clearSearches()}>
              Clear
            </button>
          </header>
          <ul className="suggest">
            {lib.searches.map((s) => (
              <li key={s}>
                <button onClick={() => run(s)}>
                  <Icon name="search" size={16} />
                  {s}
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}

      {!showSuggest && busy && <Loading label="Searching" />}
      {!showSuggest && error && <ErrorBox text={error} retry={() => run(q)} />}

      {!showSuggest && !busy && results && (
        <div className="results">
          {top && (
            <button className="top-result" onClick={() => app.openCard(top)}>
              <Art src={top.artwork} size={96} round={top.kind === "artist"} radius={12} />
              <span className="top-text">
                <span className="top-kind">{top.kind === "artist" ? "Artist" : top.kind === "album" ? "Album" : top.kind === "playlist" ? "Playlist" : "Song"}</span>
                <span className="top-title">{top.title}</span>
                <span className="top-sub">{top.subtitle}</span>
              </span>
            </button>
          )}
          {results.songs.length > 0 && (
            <Shelf title="Songs">
              {results.songs.slice(0, 8).map((t, i) => (
                <TrackRow key={t.videoId} track={t} list={results.songs} index={i} />
              ))}
            </Shelf>
          )}
          {results.artists.length > 0 && (
            <Shelf title="Artists">
              <Scroller>
                {results.artists.map((c) => (
                  <Tile key={c.id} card={c} />
                ))}
              </Scroller>
            </Shelf>
          )}
          {results.albums.length > 0 && (
            <Shelf title="Albums">
              <Scroller>
                {results.albums.map((c) => (
                  <Tile key={c.id} card={c} />
                ))}
              </Scroller>
            </Shelf>
          )}
          {results.playlists.length > 0 && (
            <Shelf title="Playlists">
              <Scroller>
                {results.playlists.map((c) => (
                  <Tile key={c.id} card={c} />
                ))}
              </Scroller>
            </Shelf>
          )}
          {results.videos.length > 0 && (
            <Shelf title="Videos">
              {results.videos.slice(0, 6).map((t, i) => (
                <TrackRow key={t.videoId} track={t} list={results.videos} index={i} />
              ))}
            </Shelf>
          )}
          {!top && !results.songs.length && !results.albums.length && <p className="empty">Nothing found for “{q}”.</p>}
        </div>
      )}
      <div className="bottom-space" />
    </div>
  );
}
