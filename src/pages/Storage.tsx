// Storage: scan a drive or folder, see where the space went, find the largest
// files and byte-identical duplicates, and move what you choose to the
// Recycle Bin after reviewing it. Nothing is removed without a review step.

import { useEffect, useMemo, useState } from "react";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Loading, Note, PageHead, Panel, Spinner } from "../components/ui";
import { api, errorMessage, type DupGroupView, type DupProgress, type FileView, type ScanProgress, type ScanSummary } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { bytes, date, duration } from "../lib/format";

function split(path: string) {
  const i = path.lastIndexOf("\\");
  return { dir: i >= 0 ? path.slice(0, i) : "", name: i >= 0 ? path.slice(i + 1) : path };
}

export function Storage() {
  const { toast } = useApp();
  const roots = useLoad(() => api.storageRoots(), []);
  const [root, setRoot] = useState<string>("");
  const [summary, setSummary] = useState<ScanSummary | null>(null);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Bumped after a scan or a removal so every view reloads.
  const [version, setVersion] = useState(0);

  useEffect(() => {
    api.storageStatus().then((s) => {
      if (s) {
        setSummary(s);
        setRoot(s.root);
      }
    }).catch(() => {});
  }, []);
  useEffect(() => {
    if (!root && roots.data?.length) setRoot(roots.data[0].path);
  }, [roots.data, root]);

  async function scan() {
    setScanning(true);
    setError(null);
    setProgress(null);
    try {
      const s = await api.storageScan(root, setProgress);
      setSummary(s);
      setVersion((v) => v + 1);
      if (s.cancelled) toast({ tone: "info", title: "Scan stopped", detail: "Results cover what was scanned before you stopped it." });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setScanning(false);
    }
  }

  return (
    <div className="page">
      <PageHead
        eyebrow="Storage"
        title="Storage"
        desc="Find out what's using your disk space: folder sizes, the largest files and exact duplicates. Anything you remove goes to the Recycle Bin after you've reviewed it."
        actions={
          <>
            <select className="select" aria-label="Drive or folder to scan" value={root} disabled={scanning} onChange={(e) => setRoot(e.target.value)} style={{ minWidth: 220 }}>
              {roots.data?.map((r) => (
                <option key={r.path} value={r.path}>
                  {r.label}{r.total ? ` — ${bytes(r.free)} free of ${bytes(r.total)}` : ""}
                </option>
              ))}
              {root && !roots.data?.some((r) => r.path === root) && <option value={root}>{root}</option>}
            </select>
            {scanning ? (
              <button className="btn" onClick={() => api.storageCancel()}><I.stop /> Stop</button>
            ) : (
              <button className="btn primary" disabled={!root} onClick={scan}><I.search /> {summary ? "Scan again" : "Scan"}</button>
            )}
          </>
        }
      />

      {error && <ErrorNote message={error} onRetry={scan} />}

      {scanning && (
        <Panel lit title="Scanning…" sub={progress?.current || root}>
          <div className="row" style={{ gap: 16 }}>
            <Spinner />
            <span className="num">{(progress?.files ?? 0).toLocaleString()} files · {(progress?.dirs ?? 0).toLocaleString()} folders · {bytes(progress?.bytes ?? 0)}</span>
          </div>
          <div className="small dim mt-2">Read-only: nothing is changed while scanning. A full system drive can take a few minutes.</div>
        </Panel>
      )}

      {!scanning && !summary && (
        <Panel>
          <Empty title="Choose a drive or folder and scan it">
            NOCTURN reads file sizes only — it doesn't open your files. Online-only OneDrive files are skipped so nothing gets downloaded.
          </Empty>
        </Panel>
      )}

      {summary && !scanning && (
        <>
          <div className="grid cols-4">
            <Stat label="Scanned" value={bytes(summary.total_size)} foot={summary.root} />
            <Stat label="Files" value={summary.total_files.toLocaleString()} foot={`${summary.folders.toLocaleString()} folders`} />
            <Stat label="Scan took" value={summary.duration_ms < 60_000 ? `${Math.max(1, Math.round(summary.duration_ms / 1000))} s` : duration(summary.duration_ms / 1000)} foot={date(summary.started_at, true)} />
            <Stat
              label="Not counted"
              value={(summary.cloud_only + summary.unreadable).toLocaleString()}
              foot={`${summary.cloud_only.toLocaleString()} online-only · ${summary.unreadable.toLocaleString()} folders without access`}
            />
          </div>
          {summary.cancelled && <Note tone="warning">This scan was stopped early, so sizes are incomplete.</Note>}

          <FolderMap version={version} />
          <LargeFiles version={version} onRemoved={() => setVersion((v) => v + 1)} />
          <Duplicates version={version} candidates={summary.dup_candidates} onRemoved={() => setVersion((v) => v + 1)} />
        </>
      )}

      <RecycleBin version={version} />
    </div>
  );
}

function Stat({ label, value, foot }: { label: string; value: string; foot: string }) {
  return (
    <div className="panel stat">
      <div className="label">{label}</div>
      <div className="value num">{value}</div>
      <div className="foot truncate" title={foot}>{foot}</div>
    </div>
  );
}

// ------------------------------------------------------------ folder map

function FolderMap({ version }: { version: number }) {
  const [id, setId] = useState<number | undefined>(undefined);
  useEffect(() => setId(undefined), [version]);
  const view = useLoad(() => api.storageFolder(id), [id, version]);
  const v = view.data;

  return (
    <section className="panel flush mt-2">
      <div className="panel-head">
        <div style={{ minWidth: 0 }}>
          <h2>Where the space goes</h2>
          <div className="sub row" style={{ gap: 4, flexWrap: "wrap" }}>
            {v?.crumbs.map(([cid, name], i) => (
              <span key={cid} className="row" style={{ gap: 4 }}>
                {i > 0 && <span className="dim">›</span>}
                {i === v.crumbs.length - 1 ? (
                  <span>{name}</span>
                ) : (
                  <button className="btn ghost sm" style={{ padding: "0 4px", height: 20 }} onClick={() => setId(cid)}>{name}</button>
                )}
              </span>
            ))}
          </div>
        </div>
        {v && <span className="small num muted">{bytes(v.size)} · {v.files.toLocaleString()} files</span>}
      </div>
      {view.error && <div style={{ padding: 16 }}><ErrorNote message={view.error} onRetry={view.reload} /></div>}
      {view.loading && !v && <Loading />}
      {v && v.children.length === 0 && v.own_files === 0 && <Empty title="Empty folder" />}
      {v && (
        <div className="table-wrap" style={{ maxHeight: 460 }}>
          <table className="table">
            <tbody>
              {v.children.map((c) => (
                <BarRow key={c.id} name={c.name} size={c.size} files={c.files} total={v.size} onOpen={c.has_children || c.files > 0 ? () => setId(c.id) : undefined} />
              ))}
              {v.own_files > 0 && <BarRow name="Files in this folder" size={v.own_size} files={v.own_files} total={v.size} muted />}
              {v.more_children > 0 && (
                <tr>
                  <td colSpan={4} className="small dim">…and {v.more_children} smaller folders</td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function BarRow({ name, size, files, total, onOpen, muted }: { name: string; size: number; files: number; total: number; onOpen?: () => void; muted?: boolean }) {
  const pct = total > 0 ? (size / total) * 100 : 0;
  return (
    <tr onClick={onOpen} style={{ cursor: onOpen ? "pointer" : "default" }}>
      <td style={{ width: "38%", maxWidth: 0 }}>
        <div className="truncate" style={{ color: muted ? "var(--ink-2)" : undefined, fontStyle: muted ? "italic" : undefined }} title={name}>
          {name}
        </div>
      </td>
      <td style={{ width: "34%" }}>
        <div style={{ height: 8, borderRadius: 4, background: "var(--bg-3)", overflow: "hidden" }} aria-hidden>
          <div style={{ width: `${Math.max(pct, size > 0 ? 0.5 : 0)}%`, height: "100%", background: "var(--series-1)", opacity: muted ? 0.5 : 0.9 }} />
        </div>
      </td>
      <td className="r num">{bytes(size)}</td>
      <td className="r num small dim">{pct >= 0.1 ? `${pct.toFixed(1)}%` : "<0.1%"} · {files.toLocaleString()} file{files === 1 ? "" : "s"}</td>
    </tr>
  );
}

// ------------------------------------------------------------ review

/** Inline review of files about to be moved to the Recycle Bin. */
function RecycleReview({ files, onCancel, onDone }: { files: FileView[]; onCancel: () => void; onDone: () => void }) {
  const { toast } = useApp();
  const [busy, setBusy] = useState(false);
  const total = files.reduce((a, f) => a + f.size, 0);
  const appData = files.filter((f) => f.app_data).length;

  async function go() {
    setBusy(true);
    try {
      const r = await api.storageRecycle(files.map((f) => f.path));
      const failed = r.results.filter((x) => !x.ok);
      toast({
        tone: failed.length ? "warning" : "good",
        title: `${r.results.length - failed.length} file${r.results.length - failed.length === 1 ? "" : "s"} moved to the Recycle Bin`,
        detail: failed.length ? `${failed.length} kept: ${failed[0].error}` : `${bytes(r.freed)} will be freed when the Recycle Bin is emptied. Restore from the Recycle Bin if you change your mind.`,
      });
      onDone();
    } catch (e) {
      toast({ tone: "critical", title: "Nothing was removed", detail: errorMessage(e) });
      setBusy(false);
    }
  }

  return (
    <div className="stack" style={{ gap: 10, padding: 16, borderTop: "1px solid var(--line)", background: "var(--bg-2)" }}>
      <div className="row between">
        <strong>Move {files.length} file{files.length === 1 ? "" : "s"} ({bytes(total)}) to the Recycle Bin?</strong>
      </div>
      <div className="table-wrap" style={{ maxHeight: 180 }}>
        <table className="table">
          <tbody>
            {files.map((f) => (
              <tr key={f.path}>
                <td className="mono small selectable" style={{ maxWidth: 0, width: "80%" }}><div className="truncate" title={f.path}>{f.path}</div></td>
                <td className="r num small">{bytes(f.size)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {appData > 0 && <Note tone="warning">{appData} of these {appData === 1 ? "is" : "are"} inside an app's data folder (AppData). Removing it can reset or break that app.</Note>}
      <div className="small dim">If a file is too big for the Recycle Bin, Windows will ask before deleting it permanently.</div>
      <div className="row" style={{ justifyContent: "flex-end", gap: 6 }}>
        <button className="btn ghost" disabled={busy} onClick={onCancel}>Cancel</button>
        <button className="btn primary" disabled={busy} onClick={go}>{busy ? <Spinner /> : null} Move to Recycle Bin</button>
      </div>
    </div>
  );
}

function FileCheck({ f, checked, onToggle }: { f: FileView; checked: boolean; onToggle: () => void }) {
  return (
    <input
      type="checkbox"
      aria-label={`Select ${split(f.path).name}`}
      checked={checked}
      disabled={!!f.protected}
      title={f.protected ?? undefined}
      onChange={onToggle}
    />
  );
}

function useSelection(version: number) {
  const [sel, setSel] = useState<Set<string>>(new Set());
  useEffect(() => setSel(new Set()), [version]);
  const toggle = (p: string) =>
    setSel((s) => {
      const n = new Set(s);
      n.has(p) ? n.delete(p) : n.add(p);
      return n;
    });
  return { sel, setSel, toggle };
}

// ------------------------------------------------------------ large files

function LargeFiles({ version, onRemoved }: { version: number; onRemoved: () => void }) {
  const files = useLoad(() => api.storageLargeFiles(), [version]);
  const { sel, setSel, toggle } = useSelection(version);
  const [reviewing, setReviewing] = useState(false);
  const chosen = (files.data ?? []).filter((f) => sel.has(f.path));

  return (
    <section className="panel flush mt-2">
      <div className="panel-head">
        <div>
          <h2>Largest files</h2>
          <div className="sub">The {files.data?.length ?? 0} biggest files in this scan. Tick files to review them for removal.</div>
        </div>
        {chosen.length > 0 && !reviewing && (
          <button className="btn primary sm" onClick={() => setReviewing(true)}>Review {chosen.length} ({bytes(chosen.reduce((a, f) => a + f.size, 0))})</button>
        )}
      </div>
      {files.error && <div style={{ padding: 16 }}><ErrorNote message={files.error} onRetry={files.reload} /></div>}
      {files.loading && !files.data && <Loading />}
      {files.data && files.data.length === 0 && <Empty title="No files" />}
      {files.data && files.data.length > 0 && (
        <div className="table-wrap" style={{ maxHeight: 460 }}>
          <table className="table">
            <thead>
              <tr>
                <th style={{ width: 28 }} />
                <th>File</th>
                <th className="r">Size</th>
                <th className="r">Modified</th>
              </tr>
            </thead>
            <tbody>
              {files.data.map((f) => {
                const { dir, name } = split(f.path);
                return (
                  <tr key={f.path}>
                    <td><FileCheck f={f} checked={sel.has(f.path)} onToggle={() => toggle(f.path)} /></td>
                    <td style={{ maxWidth: 0 }}>
                      <div className="row" style={{ gap: 6 }}>
                        <span className="truncate" title={name}>{name}</span>
                        {f.protected && <span className="tag" title={f.protected}><I.lock /> Protected</span>}
                        {f.app_data && !f.protected && <span className="tag warning">App data</span>}
                      </div>
                      <div className="small dim mono truncate selectable" title={dir}>{dir}</div>
                    </td>
                    <td className="r num">{bytes(f.size)}</td>
                    <td className="r small muted">{f.modified ? date(new Date(f.modified).toISOString()) : "—"}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {reviewing && chosen.length > 0 && (
        <RecycleReview
          files={chosen}
          onCancel={() => setReviewing(false)}
          onDone={() => {
            setReviewing(false);
            setSel(new Set());
            onRemoved();
          }}
        />
      )}
    </section>
  );
}

// ------------------------------------------------------------ duplicates

function Duplicates({ version, candidates, onRemoved }: { version: number; candidates: number; onRemoved: () => void }) {
  const [groups, setGroups] = useState<DupGroupView[] | null>(null);
  const [progress, setProgress] = useState<DupProgress | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { sel, setSel, toggle } = useSelection(version);
  const [reviewing, setReviewing] = useState(false);

  // Cached results survive navigation; a new scan clears them on the backend.
  useEffect(() => {
    api.storageDuplicates().then(setGroups).catch(() => {});
  }, [version]);

  async function find() {
    setRunning(true);
    setError(null);
    setProgress(null);
    try {
      setGroups(await api.storageFindDuplicates(setProgress));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setRunning(false);
    }
  }

  const all = useMemo(() => (groups ?? []).flatMap((g) => g.files), [groups]);
  const chosen = all.filter((f) => sel.has(f.path));
  const wasted = (groups ?? []).reduce((a, g) => a + g.wasted, 0);
  // Every group must keep at least one copy.
  const keepsOne = (groups ?? []).every((g) => g.files.some((f) => !sel.has(f.path)));

  function selectAllButNewest() {
    const n = new Set<string>();
    for (const g of groups ?? []) {
      const keep = [...g.files].sort((a, b) => (b.modified ?? 0) - (a.modified ?? 0))[0];
      for (const f of g.files) if (f !== keep && !f.protected) n.add(f.path);
    }
    setSel(n);
  }

  return (
    <section className="panel flush mt-2">
      <div className="panel-head">
        <div>
          <h2>Duplicate files</h2>
          <div className="sub">
            {groups
              ? `${groups.length} set${groups.length === 1 ? "" : "s"} of identical files · ${bytes(wasted)} in extra copies`
              : `Compares the contents of ${candidates.toLocaleString()} files of 1 MB or more that share a size with another file`}
          </div>
        </div>
        <span className="row" style={{ gap: 6 }}>
          {groups && groups.length > 0 && <button className="btn ghost sm" onClick={selectAllButNewest}>Select all but the newest copy</button>}
          {chosen.length > 0 && !reviewing && (
            <button className="btn primary sm" disabled={!keepsOne} onClick={() => setReviewing(true)}>Review {chosen.length} ({bytes(chosen.reduce((a, f) => a + f.size, 0))})</button>
          )}
          {running ? (
            <button className="btn sm" onClick={() => api.storageCancel()}><I.stop /> Stop</button>
          ) : (
            <button className="btn sm" disabled={candidates === 0} onClick={find}>{groups ? "Search again" : "Find duplicates"}</button>
          )}
        </span>
      </div>
      {error && <div style={{ padding: 16 }}><ErrorNote message={error} /></div>}
      {running && (
        <div style={{ padding: 16 }} className="stack">
          <div className="row between small">
            <span><Spinner /> Comparing file contents…</span>
            <span className="num">{bytes(progress?.hashed_bytes ?? 0)} of {bytes(progress?.total_bytes ?? 0)}</span>
          </div>
          <div style={{ height: 6, borderRadius: 3, background: "var(--bg-3)", overflow: "hidden" }}>
            <div style={{ width: `${progress && progress.total_bytes ? (progress.hashed_bytes / progress.total_bytes) * 100 : 0}%`, height: "100%", background: "var(--series-1)" }} />
          </div>
        </div>
      )}
      {!keepsOne && <div style={{ padding: "0 16px" }}><Note tone="warning">Leave at least one copy unticked in every set.</Note></div>}
      {groups && groups.length === 0 && !running && <Empty title="No duplicates">No two files of 1 MB or more in this scan have identical contents.</Empty>}
      {groups && groups.length > 0 && (
        <div className="table-wrap" style={{ maxHeight: 520 }}>
          <table className="table">
            <tbody>
              {groups.map((g, gi) => (
                <DupRows key={gi} g={g} sel={sel} toggle={toggle} />
              ))}
            </tbody>
          </table>
        </div>
      )}
      {reviewing && chosen.length > 0 && keepsOne && (
        <RecycleReview
          files={chosen}
          onCancel={() => setReviewing(false)}
          onDone={() => {
            setReviewing(false);
            setSel(new Set());
            onRemoved();
          }}
        />
      )}
    </section>
  );
}

function DupRows({ g, sel, toggle }: { g: DupGroupView; sel: Set<string>; toggle: (p: string) => void }) {
  return (
    <>
      <tr>
        <td colSpan={3} className="small" style={{ background: "var(--bg-2)" }}>
          <strong>{split(g.files[0].path).name}</strong> <span className="dim">· {g.files.length} copies of {bytes(g.size)} · {bytes(g.wasted)} extra</span>
        </td>
      </tr>
      {g.files.map((f) => (
        <tr key={f.path}>
          <td style={{ width: 28 }}><FileCheck f={f} checked={sel.has(f.path)} onToggle={() => toggle(f.path)} /></td>
          <td className="mono small selectable" style={{ maxWidth: 0 }}>
            <div className="row" style={{ gap: 6 }}>
              <span className="truncate" title={f.path}>{f.path}</span>
              {f.protected && <span className="tag" title={f.protected}><I.lock /> Protected</span>}
              {f.app_data && !f.protected && <span className="tag warning">App data</span>}
            </div>
          </td>
          <td className="r small muted">{f.modified ? date(new Date(f.modified).toISOString()) : "—"}</td>
        </tr>
      ))}
    </>
  );
}

// ------------------------------------------------------------ recycle bin

function RecycleBin({ version }: { version: number }) {
  const { toast } = useApp();
  const info = useLoad(() => api.recycleBinInfo(), [version]);
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);

  async function empty() {
    setBusy(true);
    try {
      await api.emptyRecycleBin();
      toast({ tone: "good", title: "Recycle Bin emptied", detail: info.data ? `${bytes(info.data.size)} freed` : undefined });
      info.reload();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't empty the Recycle Bin", detail: errorMessage(e) });
    } finally {
      setBusy(false);
      setConfirm(false);
    }
  }

  return (
    <Panel className="mt-2" title="Recycle Bin" sub="Files you remove stay here until it's emptied">
      {info.error && <ErrorNote message={info.error} onRetry={info.reload} />}
      {info.data && (
        <div className="row between">
          <span className="num">{info.data.items.toLocaleString()} item{info.data.items === 1 ? "" : "s"} · {bytes(info.data.size)}</span>
          {info.data.items > 0 &&
            (confirm ? (
              <span className="row" style={{ gap: 6 }}>
                <span className="small" style={{ color: "#ee8a8a" }}>Permanently delete everything in it?</span>
                <button className="btn ghost sm" disabled={busy} onClick={() => setConfirm(false)}>Keep</button>
                <button className="btn sm" style={{ borderColor: "rgba(208,59,59,.5)" }} disabled={busy} onClick={empty}>{busy ? <Spinner /> : null} Empty it</button>
              </span>
            ) : (
              <button className="btn sm" onClick={() => setConfirm(true)}>Empty Recycle Bin…</button>
            ))}
        </div>
      )}
    </Panel>
  );
}
