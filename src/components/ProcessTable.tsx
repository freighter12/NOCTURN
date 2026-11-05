// Grouped process view with a two-step "End" action. Core Windows processes
// are refused by the backend regardless of what the UI sends.

import { useEffect, useMemo, useState } from "react";
import { api, errorMessage, type ProcessInfo } from "../lib/api";
import { useApp } from "../lib/app";
import { bytes } from "../lib/format";
import { Seg } from "./ui";

interface Group {
  name: string;
  cpu: number;
  memory: number;
  disk: number;
  pids: ProcessInfo[];
}

const PROTECTED = new Set(["system", "registry", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe", "lsass.exe", "svchost.exe", "dwm.exe", "msmpeng.exe", "nocturn.exe", "memcompression", "fontdrvhost.exe", "audiodg.exe", "conhost.exe", "sihost.exe", "ctfmon.exe", "runtimebroker.exe", "taskhostw.exe", "searchindexer.exe", "spoolsv.exe", "secure system", "wudfhost.exe", "lsaiso.exe", "nissrv.exe", "securityhealthservice.exe", "sgrmbroker.exe"]);

export function ProcessTable({ limit = 12, initialSort = "cpu" as "cpu" | "memory" | "disk" }) {
  const { toast } = useApp();
  const [procs, setProcs] = useState<ProcessInfo[]>([]);
  const [sort, setSort] = useState(initialSort);
  const [confirm, setConfirm] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const load = () => api.processes().then((p) => alive && setProcs(p)).catch(() => {});
    load();
    const t = setInterval(load, 3000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  const groups = useMemo(() => {
    const m = new Map<string, Group>();
    for (const p of procs) {
      if (p.pid === 0) continue;
      const g = m.get(p.name) ?? { name: p.name, cpu: 0, memory: 0, disk: 0, pids: [] };
      g.cpu += p.cpu;
      g.memory += p.memory;
      g.disk += p.disk_read_bps + p.disk_write_bps;
      g.pids.push(p);
      m.set(p.name, g);
    }
    const arr = [...m.values()];
    arr.sort((a, b) => (sort === "cpu" ? b.cpu - a.cpu : sort === "memory" ? b.memory - a.memory : b.disk - a.disk));
    return arr.slice(0, limit);
  }, [procs, sort, limit]);

  async function end(g: Group) {
    let ok = 0;
    let lastErr = "";
    for (const p of g.pids) {
      try {
        await api.endProcess(p.pid, p.name);
        ok++;
      } catch (e) {
        lastErr = errorMessage(e);
      }
    }
    setConfirm(null);
    toast(ok ? { tone: "good", title: `Ended ${g.name}`, detail: `${ok} of ${g.pids.length} process(es)` } : { tone: "critical", title: `Couldn't end ${g.name}`, detail: lastErr });
  }

  return (
    <div>
      <div className="row between" style={{ padding: "0 18px 10px" }}>
        <span className="small dim">{procs.length ? `${procs.length} processes · grouped by name · refreshes every 3 s` : "Loading…"}</span>
        <Seg value={sort} onChange={setSort} options={[{ value: "cpu", label: "CPU" }, { value: "memory", label: "Memory" }, { value: "disk", label: "Disk" }]} />
      </div>
      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th>Process</th>
              <th className="r">CPU</th>
              <th className="r">Memory</th>
              <th className="r">Disk</th>
              <th className="r" style={{ width: 150 }}></th>
            </tr>
          </thead>
          <tbody>
            {groups.map((g) => {
              const prot = PROTECTED.has(g.name.toLowerCase());
              return (
                <tr key={g.name}>
                  <td>
                    <div className="truncate" style={{ maxWidth: 360 }} title={g.pids[0]?.exe ?? g.name}>
                      {g.name} {g.pids.length > 1 && <span className="dim">×{g.pids.length}</span>}
                    </div>
                  </td>
                  <td className="r num">{g.cpu.toFixed(1)}%</td>
                  <td className="r num">{bytes(g.memory)}</td>
                  <td className="r num">{g.disk ? `${bytes(g.disk)}/s` : "—"}</td>
                  <td className="r">
                    {prot ? (
                      <span className="dim small">Windows</span>
                    ) : confirm === g.name ? (
                      <span className="row" style={{ justifyContent: "flex-end", gap: 6 }}>
                        <button className="btn ghost sm" onClick={() => setConfirm(null)}>
                          Keep
                        </button>
                        <button className="btn danger sm" onClick={() => end(g)}>
                          End{g.pids.length > 1 ? " all" : ""}
                        </button>
                      </span>
                    ) : (
                      <button className="btn ghost sm" onClick={() => setConfirm(g.name)} title="Unsaved work in this app will be lost">
                        End…
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
