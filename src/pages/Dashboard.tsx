import { useEffect, useState } from "react";
import { api, onEngineChange } from "../api";
import type { JournalEntry, Rule } from "../types";
import type { PageKey } from "../components/Nav";
import { humanBytes } from "../components/Bytes";
import { useT } from "../i18n";

export function Dashboard({ onNavigate }: { onNavigate: (k: PageKey) => void }) {
  const { t } = useT();
  const [rules, setRules] = useState<Rule[]>([]);
  const [entries, setEntries] = useState<JournalEntry[]>([]);

  const [reviewCount, setReviewCount] = useState(0);

  useEffect(() => {
    const load = () => {
      api.listRules().then(setRules);
      api.journal(50).then(setEntries);
      api.reviewQueue().then((q) => setReviewCount(q.length));
    };
    load();
    let off = () => {};
    onEngineChange(load).then((fn) => (off = fn));
    return () => off();
  }, []);

  const today = new Date().toDateString();
  const todays = entries.filter(
    (e) => new Date(e.started_at).toDateString() === today,
  );
  const organized = todays.filter((e) => e.state === "completed").length;
  const bytes = todays
    .filter((e) => e.state === "completed")
    .reduce((s, e) => s + e.size, 0);
  const activeRules = rules.filter((r) => r.enabled).length;

  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">{t("dashboard.title")}</h1>
        <p className="mt-1 text-sm text-text-muted">
          {t("dashboard.subtitle")}
        </p>
      </header>

      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
        <Stat label={t("dashboard.organized")} value={String(organized)} />
        <Stat label={t("dashboard.bytes")} value={humanBytes(bytes)} />
        <Stat label={t("dashboard.review")} value={String(reviewCount)} />
      </div>

      <div className="card p-4">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-sm text-text-muted">
              {t("dashboard.active_rules")}
            </div>
            <div className="text-2xl font-semibold">{activeRules}</div>
          </div>
          <button className="btn" onClick={() => onNavigate("rules")}>
            {t("dashboard.manage")}
          </button>
        </div>
      </div>

      <div className="card p-4">
        <div className="mb-3 flex items-center justify-between">
          <div className="text-sm text-text-muted">
            {t("dashboard.recent")}
          </div>
          <button className="btn-ghost" onClick={() => onNavigate("activity")}>
            {t("dashboard.view_all")}
          </button>
        </div>
        {entries.length === 0 ? (
          <div className="rounded-md border border-dashed border-border p-6 text-center text-sm text-text-faint">
            {t("dashboard.empty")}
          </div>
        ) : (
          <ul className="divide-y divide-border">
            {entries.slice(0, 6).map((e) => (
              <li key={e.op_id} className="py-2 text-sm">
                <div className="font-mono text-text">
                  {e.kind} · {truncatePath(e.source)} →{" "}
                  {truncatePath(e.destination)}
                </div>
                <div className="text-xs text-text-faint">
                  {new Date(e.started_at).toLocaleString()} · {e.rule_name}
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="card p-4">
      <div className="text-xs uppercase tracking-wide text-text-faint">
        {label}
      </div>
      <div className="mt-1 text-2xl font-semibold">{value}</div>
    </div>
  );
}

function truncatePath(p: string | null): string {
  if (!p) return "—";
  if (p.length <= 60) return p;
  return "…" + p.slice(p.length - 57);
}
