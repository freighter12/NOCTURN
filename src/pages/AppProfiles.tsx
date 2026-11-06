// App profiles: while a chosen program runs, use a power plan and priority
// and hold scheduled maintenance; switch back when it closes.

import { useEffect, useState } from "react";
import { I } from "../components/Icons";
import { Empty, ErrorNote, Loading, Note, PageHead, Panel, Spinner, Toggle } from "../components/ui";
import { api, errorMessage, type AppProfile, type Priority } from "../lib/api";
import { useApp, useLoad } from "../lib/app";
import { relative } from "../lib/format";

const PRIORITY: { value: Priority | ""; label: string }[] = [
  { value: "", label: "Don't change" },
  { value: "above_normal", label: "Above normal" },
  { value: "high", label: "High" },
  { value: "below_normal", label: "Below normal" },
  { value: "idle", label: "Low (idle)" },
];
const priorityLabel = (p: Priority | null) => PRIORITY.find((x) => x.value === (p ?? ""))?.label ?? p;

const EMPTY: AppProfile = { exe: "", label: "", power_plan: null, priority: null, pause_maintenance: true, enabled: true };

export function AppProfiles() {
  const { toast, navigate } = useApp();
  const data = useLoad(() => api.appProfiles(), []);
  const plans = useLoad(() => api.powerPlans(), []);
  const [form, setForm] = useState<AppProfile | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Keep the "active now" badge fresh.
  useEffect(() => {
    const t = setInterval(() => api.appProfiles().then(data.setData).catch(() => {}), 5000);
    return () => clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const planName = (g: string | null) => (g ? plans.data?.find((p) => p.guid.toLowerCase() === g.toLowerCase())?.name ?? "unknown plan" : "Don't change");

  async function submit() {
    if (!form) return;
    setBusy(true);
    setError(null);
    try {
      await api.saveAppProfile({ ...form, label: form.label.trim() || form.exe.replace(/\.exe$/i, "") });
      toast({ tone: "good", title: "Profile saved", detail: `It switches on the next time ${form.exe} starts.` });
      setForm(null);
      data.reload();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function toggle(p: AppProfile, enabled: boolean) {
    try {
      await api.saveAppProfile({ ...p, enabled });
      data.reload();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't update", detail: errorMessage(e) });
    }
  }

  async function remove(p: AppProfile) {
    try {
      await api.deleteAppProfile(p.exe);
      data.reload();
    } catch (e) {
      toast({ tone: "critical", title: "Couldn't delete", detail: errorMessage(e) });
    }
  }

  const active = data.data?.active ?? null;

  return (
    <div className="page">
      <PageHead
        eyebrow="App profiles"
        title="Settings that follow your apps"
        desc="Pick a program and what should change while it runs. NOCTURN switches when it starts and switches back when it closes. Every power-plan switch is recorded in History."
        actions={!form && <button className="btn primary" onClick={() => { setError(null); setForm({ ...EMPTY }); }}><I.plus /> New profile</button>}
      />

      {active && (
        <Note tone="good">
          <b>{active.label}</b> is active — {active.exe} has been running since {relative(active.since)}.
          {active.pause_maintenance ? " Scheduled maintenance is on hold." : ""}
        </Note>
      )}

      {form && (
        <Panel className="mt-2" title={data.data?.profiles.some((p) => p.exe.toLowerCase() === form.exe.toLowerCase()) ? `Edit ${form.label || form.exe}` : "New profile"}>
          <div className="grid cols-2" style={{ gap: 14 }}>
            <div className="field">
              <label htmlFor="pexe">Program</label>
              <input id="pexe" className="input" list="papps" placeholder="e.g. Blender.exe" value={form.exe} onChange={(e) => setForm({ ...form, exe: e.target.value.trim() })} />
              <datalist id="papps">{(data.data?.running_apps ?? []).map((n) => <option key={n} value={n} />)}</datalist>
              <div className="hint">The program's file name. Apps running now are suggested.</div>
            </div>
            <div className="field">
              <label htmlFor="plabel">Name</label>
              <input id="plabel" className="input" placeholder="e.g. 3D rendering" maxLength={80} value={form.label} onChange={(e) => setForm({ ...form, label: e.target.value })} />
            </div>
            <div className="field">
              <label htmlFor="pplan">Power plan while it runs</label>
              <select id="pplan" className="select" value={form.power_plan ?? ""} onChange={(e) => setForm({ ...form, power_plan: e.target.value || null })}>
                <option value="">Don't change</option>
                {(plans.data ?? []).map((p) => <option key={p.guid} value={p.guid}>{p.name}</option>)}
              </select>
              <div className="hint">Your previous plan comes back when the program closes — unless you change it yourself meanwhile.</div>
            </div>
            <div className="field">
              <label htmlFor="ppri">Priority</label>
              <select id="ppri" className="select" value={form.priority ?? ""} onChange={(e) => setForm({ ...form, priority: (e.target.value || null) as Priority | null })}>
                {PRIORITY.map((p) => <option key={p.value} value={p.value}>{p.label}</option>)}
              </select>
              <div className="hint">Applied each time it starts. Realtime is never used.</div>
            </div>
          </div>
          <label className="row mt-2" style={{ gap: 8 }}>
            <input type="checkbox" className="check" checked={form.pause_maintenance} onChange={(e) => setForm({ ...form, pause_maintenance: e.target.checked })} />
            Hold scheduled maintenance while it runs
          </label>
          {error && <div className="mt-2"><ErrorNote message={error} /></div>}
          <div className="row mt-2" style={{ justifyContent: "flex-end", gap: 8 }}>
            <button className="btn ghost" disabled={busy} onClick={() => setForm(null)}>Cancel</button>
            <button className="btn primary" disabled={busy || !form.exe} onClick={submit}>{busy ? <Spinner /> : null} Save profile</button>
          </div>
        </Panel>
      )}

      <section className="panel flush mt-2">
        {data.loading && !data.data && <Loading />}
        {data.error && <div style={{ padding: 16 }}><ErrorNote message={data.error} onRetry={data.reload} /></div>}
        {data.data && data.data.profiles.length === 0 && (
          <Empty title="No app profiles yet">For example: while your video editor runs, use High performance and hold maintenance; while a game launcher runs, raise its priority.</Empty>
        )}
        {data.data && data.data.profiles.length > 0 && (
          <table className="table">
            <thead><tr><th>Profile</th><th>Power plan</th><th>Priority</th><th>Maintenance</th><th style={{ width: 60 }}>On</th><th className="r" style={{ width: 120 }} /></tr></thead>
            <tbody>
              {data.data.profiles.map((p) => (
                <tr key={p.exe}>
                  <td>
                    <div className="row" style={{ gap: 8 }}>
                      <span style={{ fontWeight: 500 }}>{p.label}</span>
                      {active?.exe.toLowerCase() === p.exe.toLowerCase() && <span className="tag good">Active now</span>}
                    </div>
                    <div className="small mono dim">{p.exe}</div>
                  </td>
                  <td className="small">{planName(p.power_plan)}</td>
                  <td className="small">{priorityLabel(p.priority)}</td>
                  <td className="small">{p.pause_maintenance ? "Held" : "Runs as usual"}</td>
                  <td><Toggle on={p.enabled} label={`Profile ${p.label} enabled`} onChange={(v) => toggle(p, v)} /></td>
                  <td className="r" style={{ whiteSpace: "nowrap" }}>
                    <button className="btn ghost sm" onClick={() => { setError(null); setForm({ ...p }); }}>Edit</button>
                    <button className="btn ghost sm icon" aria-label={`Delete ${p.label}`} onClick={() => remove(p)}><I.close /></button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <div className="mt-2">
        <Note>
          Games have their own profiles on the <a href="#" onClick={(e) => { e.preventDefault(); navigate("gaming"); }}>Gaming</a> page; this page is for any program. Only one profile is active at a time (the first matching program). Profiles work while NOCTURN is open or minimised.
        </Note>
      </div>
    </div>
  );
}
