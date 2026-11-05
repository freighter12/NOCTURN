// Startup Manager: every Run-key and Startup-folder entry, switched on/off the
// same way Task Manager does (StartupApproved), so nothing is deleted.

import { useMemo, useState } from "react";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Loading, Note, PageHead, Panel, Seg, Spinner, StatusTag, Toggle } from "../components/ui";
import { api, errorMessage, type StartupItem } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { relative } from "../lib/format";

type Filter = "all" | "enabled" | "disabled" | "review";

function SignatureTag({ s }: { s: StartupItem }) {
  const i = s.insight;
  if (!i || !i.signature) return null;
  if (i.windows_component) return <span className="tag" title="Part of Windows (catalog-signed by Microsoft)">Windows</span>;
  if (i.signature === "valid") return <span className="tag good" title="Digitally signed, and the signature verifies: the file hasn't been altered since it was signed"><I.check /> Signed</span>;
  if (i.signature === "invalid") return <span className="tag critical" title="A signature is present but doesn't verify">Signature invalid</span>;
  if (i.signature === "unsigned") return <span className="tag" title="No digital signature. Common for small or open-source tools.">Unsigned</span>;
  return null;
}

function askPrompt(s: StartupItem) {
  const i = s.insight;
  return `Is it safe to disable "${s.name}" from starting with Windows? What does it do, and what would I lose? (Startup entry ID: ${s.id}. Publisher: ${i?.publisher ?? "unknown"}. It runs: ${s.command})`;
}

export function Startup() {
  const { dataVersion, bumpData, toast, navigate, requestApply } = useApp();
  const items = useLoad(() => api.startup(), [dataVersion]);
  const [filter, setFilter] = useState<Filter>("all");
  const [q, setQ] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());

  const rows = useMemo(
    () =>
      (items.data ?? [])
        .filter((s) => (filter === "all" ? true : filter === "enabled" ? s.enabled : filter === "review" ? (s.insight?.review.length ?? 0) > 0 : !s.enabled))
        .filter((s) => !q || `${s.name} ${s.command}`.toLowerCase().includes(q.toLowerCase()))
        .sort((a, b) => Number(b.enabled) - Number(a.enabled) || a.name.localeCompare(b.name)),
    [items.data, filter, q],
  );
  const enabledCount = items.data?.filter((s) => s.enabled).length ?? 0;
  const missing = items.data?.filter((s) => s.file_exists === false) ?? [];
  const review = items.data?.filter((s) => (s.insight?.review.length ?? 0) > 0) ?? [];
  const changes = useLoad(() => api.startupChanges(), [dataVersion]);

  async function setOne(s: StartupItem, enabled: boolean) {
    setBusy(s.id);
    try {
      const r = await api.setStartup([s.id], enabled);
      const res = r.results[0];
      const ok = r.status === "completed" || res?.status === "applied" || res?.status === "unchanged";
      toast({
        tone: ok ? "good" : "warning",
        title: ok ? `${s.name} ${enabled ? "enabled" : "disabled"}` : `Couldn't change ${s.name}`,
        detail: ok ? "Takes effect at next sign-in. Recorded in History." : res?.error ?? r.status,
        action: ok ? { label: "Undo", run: () => api.rollback(r.tx_id).then(bumpData).catch((e) => toast({ tone: "critical", title: "Undo failed", detail: errorMessage(e) })) } : undefined,
      });
      bumpData();
    } catch (e) {
      toast({ tone: "critical", title: `Couldn't change ${s.name}`, detail: errorMessage(e) });
    } finally {
      setBusy(null);
    }
  }

  const toggleSel = (id: string) =>
    setSelected((cur) => {
      const n = new Set(cur);
      n.has(id) ? n.delete(id) : n.add(id);
      return n;
    });

  return (
    <div className="page">
      <PageHead
        eyebrow="Startup Manager"
        title="Startup apps"
        desc="Programs that launch when you sign in. Disabling one stops it starting automatically — it isn't uninstalled or deleted, and you can switch it back on at any time."
        actions={
          <button className="btn" onClick={() => navigate("assistant", { prompt: "Find unnecessary startup applications on my PC and explain which ones are safe to disable." })}>
            <I.assistant /> Ask AI which to disable
          </button>
        }
      />

      <div className="grid cols-3">
        <div className="panel stat">
          <div className="label">Enabled at sign-in</div>
          <div className="value num">{items.data ? enabledCount : "—"}</div>
          <div className="foot">of {items.data?.length ?? "—"} entries found</div>
        </div>
        <div className="panel stat">
          <div className="label">Need administrator approval</div>
          <div className="value num">{items.data ? items.data.filter((s) => s.requires_admin).length : "—"}</div>
          <div className="foot">Machine-wide entries affect every user</div>
        </div>
        <div className="panel stat">
          <div className="label">Point to missing files</div>
          <div className="value num">{items.data ? missing.length : "—"}</div>
          <div className="foot">Usually left behind by uninstalled apps</div>
        </div>
      </div>

      <section className="panel flush mt-2">
        <div className="row between" style={{ padding: "12px 16px", borderBottom: "1px solid var(--line)" }}>
          <div className="row">
            <Seg<Filter> value={filter} onChange={setFilter} options={[{ value: "all", label: "All" }, { value: "enabled", label: "Enabled" }, { value: "disabled", label: "Disabled" }, { value: "review", label: `Worth reviewing${review.length ? ` (${review.length})` : ""}` }]} />
            <input className="input" style={{ width: 220, height: 30 }} placeholder="Search name or command" value={q} onChange={(e) => setQ(e.target.value)} />
          </div>
          <div className="row">
            <button className="btn sm icon" onClick={items.reload} aria-label="Refresh"><I.refresh /></button>
            <button
              className="btn primary sm"
              disabled={!selected.size}
              onClick={() =>
                requestApply({
                  tweakIds: [],
                  startupDisableIds: [...selected],
                  source: "user",
                  label: `Disable ${selected.size} startup app(s)`,
                  title: "Disable startup apps",
                  onDone: (o) => o.approved && setSelected(new Set()),
                })
              }
            >
              Disable {selected.size || ""} selected
            </button>
          </div>
        </div>
        {items.loading && !items.data && <Loading label="Reading startup entries and checking signatures…" />}
        {items.error && <div style={{ padding: 16 }}><ErrorNote message={items.error} onRetry={items.reload} /></div>}
        {items.data && rows.length === 0 && <Empty title="No matching startup entries" />}
        {rows.length > 0 && (
          <table className="table">
            <thead>
              <tr>
                <th style={{ width: 36 }} />
                <th>App</th>
                <th>Location</th>
                <th className="r" style={{ width: 70 }} />
                <th className="r" style={{ width: 110 }}>At sign-in</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((s) => (
                <tr key={s.id}>
                  <td>
                    <input type="checkbox" className="check" checked={selected.has(s.id)} disabled={!s.enabled} onChange={() => toggleSel(s.id)} aria-label={`Select ${s.name}`} />
                  </td>
                  <td style={{ maxWidth: 0, width: "60%" }}>
                    <div className="row" style={{ gap: 8, flexWrap: "wrap" }}>
                      <span style={{ fontWeight: 500 }}>{s.name}</span>
                      <SignatureTag s={s} />
                      {s.requires_admin && <span className="tag" title="Machine-wide entry; Windows will ask for administrator approval"><I.lock /> Admin</span>}
                      {s.file_exists === false && <span className="tag warning" title="The program this entry launches no longer exists">File missing</span>}
                    </div>
                    {(s.insight?.publisher || s.insight?.description) && (
                      <div className="small muted truncate" title="As stated by the program file itself">
                        {[s.insight.description, s.insight.publisher].filter(Boolean).join(" · ")}
                      </div>
                    )}
                    <div className="mono dim truncate selectable" title={s.command}>{s.command}</div>
                    {s.insight?.review.map((r) => (
                      <div key={r} className="small" style={{ color: "#f6cc6e", marginTop: 2 }}>
                        {r}
                      </div>
                    ))}
                  </td>
                  <td className="small muted">{s.location_label}</td>
                  <td className="r">
                    <button className="btn ghost sm" title="Ask NOCTURN AI what this is and whether to disable it" onClick={() => navigate("assistant", { prompt: askPrompt(s) })}>
                      <I.assistant /> Ask
                    </button>
                  </td>
                  <td className="r">
                    <span className="row" style={{ justifyContent: "flex-end", gap: 8 }}>
                      {busy === s.id && <Spinner />}
                      <Toggle on={s.enabled} disabled={!!busy} onChange={(v) => setOne(s, v)} label={`${s.name} starts at sign-in`} />
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
      <Panel className="mt-2" title="Changes made by NOCTURN" sub="Every startup switch is journalled. Undo any of them from History." actions={<button className="btn ghost sm" onClick={() => navigate("history")}>Open History <I.arrow /></button>}>
        {changes.error && <ErrorNote message={changes.error} onRetry={changes.reload} />}
        {changes.data && changes.data.length === 0 && <Empty title="No startup changes yet" />}
        {changes.data && changes.data.length > 0 && (
          <div className="list">
            {changes.data.slice(0, 8).map((c, i) => (
              <div key={i} className="list-item" style={{ alignItems: "center" }}>
                <div className="body">
                  <div className="title">{c.target}</div>
                  <div className="desc">
                    {relative(c.applied_at)} · {c.source === "ai" ? "approved from an AI plan" : "by you"}
                    {c.rolled_back_at ? ` · undone ${relative(c.rolled_back_at)}` : ""}
                  </div>
                </div>
                <StatusTag status={c.status} />
              </div>
            ))}
          </div>
        )}
      </Panel>
      <div className="mt-2">
        <Note>
          Publisher and description come from each program file and are self-reported; "Signed" means the file's digital signature verifies. Changes take effect the next time you sign in. Each switch is journalled in History and can be undone. NOCTURN doesn't touch scheduled tasks or services here — those are listed under System Diagnostics.
        </Note>
      </div>
    </div>
  );
}
