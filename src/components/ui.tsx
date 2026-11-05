import type { ReactNode } from "react";
import { ToneIcon } from "../lib/app";
import { riskLabel, statusLabel } from "../lib/format";
import type { Risk } from "../lib/api";
import { I } from "./Icons";

export function PageHead({ eyebrow, title, desc, actions }: { eyebrow?: string; title: string; desc?: ReactNode; actions?: ReactNode }) {
  return (
    <header className="page-head">
      <div>
        {eyebrow && <div className="eyebrow">{eyebrow}</div>}
        <h1>{title}</h1>
        {desc && <p>{desc}</p>}
      </div>
      {actions && <div className="actions">{actions}</div>}
    </header>
  );
}

export function Panel({ title, sub, actions, children, className = "", lit }: { title?: ReactNode; sub?: ReactNode; actions?: ReactNode; children: ReactNode; className?: string; lit?: boolean }) {
  return (
    <section className={`panel ${lit ? "lit" : ""} ${className}`}>
      {(title || actions) && (
        <div className="panel-head">
          <div>
            {title && <h2>{title}</h2>}
            {sub && <div className="sub">{sub}</div>}
          </div>
          {actions}
        </div>
      )}
      {children}
    </section>
  );
}

export function Spinner({ lg }: { lg?: boolean }) {
  return <span className={`spinner ${lg ? "lg" : ""}`} role="progressbar" aria-label="Loading" />;
}

export function Loading({ label = "Reading your system…" }: { label?: string }) {
  return (
    <div className="empty">
      <Spinner lg />
      <div className="small">{label}</div>
    </div>
  );
}

export function ErrorNote({ message, onRetry }: { message: string; onRetry?: () => void }) {
  return (
    <div className="note critical">
      <ToneIcon tone="critical" />
      <div style={{ flex: 1 }}>{message}</div>
      {onRetry && (
        <button className="btn sm" onClick={onRetry}>
          Retry
        </button>
      )}
    </div>
  );
}

export function Note({ tone = "info", children }: { tone?: "info" | "warning" | "critical" | "good"; children: ReactNode }) {
  return (
    <div className={`note ${tone}`}>
      <ToneIcon tone={tone} />
      <div style={{ flex: 1 }}>{children}</div>
    </div>
  );
}

export function Empty({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="empty">
      <div className="title">{title}</div>
      {children && <div className="small">{children}</div>}
    </div>
  );
}

export function Toggle({ on, onChange, disabled, label }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean; label: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      className={`toggle ${on ? "on" : ""}`}
      onClick={() => onChange(!on)}
    />
  );
}

export function RiskTag({ risk }: { risk: Risk }) {
  const tone = risk === "safe" ? "good" : risk === "moderate" ? "warning" : "serious";
  return (
    <span className={`tag ${tone}`} title={riskHint[risk]}>
      {risk === "safe" ? <I.shield /> : <ToneIcon tone="warning" />}
      {riskLabel[risk]}
    </span>
  );
}

const riskHint: Record<Risk, string> = {
  safe: "Reversible, documented, no meaningful downside for most PCs",
  moderate: "Reversible, but trades something (battery life, a Windows feature)",
  advanced: "Reversible; effect depends on hardware/drivers — measure before and after",
};

const statusTone: Record<string, string> = {
  applied: "good",
  completed: "good",
  rolled_back: "good",
  unchanged: "",
  nothing_to_do: "",
  not_reversible: "",
  failed: "critical",
  rollback_failed: "critical",
  skipped: "warning",
  partial: "warning",
  partially_rolled_back: "warning",
  unknown: "warning",
  interrupted: "warning",
  pending: "",
  in_progress: "",
};

export function StatusTag({ status }: { status: string }) {
  const tone = statusTone[status] ?? "";
  return (
    <span className={`tag ${tone}`}>
      {tone === "good" && <I.check />}
      {(tone === "critical" || tone === "warning") && <ToneIcon tone="warning" />}
      {statusLabel[status] ?? status}
    </span>
  );
}

export function Meter({ value, max = 100, warnAt = 80, critAt = 92, label }: { value: number; max?: number; warnAt?: number; critAt?: number; label: string }) {
  const p = Math.max(0, Math.min(100, (value / max) * 100));
  const cls = p >= critAt ? "crit" : p >= warnAt ? "warn" : "";
  return (
    <div className={`meter ${cls}`} role="meter" aria-label={label} aria-valuenow={Math.round(p)} aria-valuemin={0} aria-valuemax={100}>
      <span style={{ width: `${p}%` }} />
    </div>
  );
}

export function Seg<T extends string>({ value, options, onChange }: { value: T; options: { value: T; label: string }[]; onChange: (v: T) => void }) {
  return (
    <div className="seg" role="tablist">
      {options.map((o) => (
        <button key={o.value} role="tab" aria-selected={value === o.value} className={value === o.value ? "on" : ""} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}
