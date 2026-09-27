import { useEffect, useState } from "react";
import { api } from "../api";
import type { JournalEntry } from "../types";
import { humanBytes } from "../components/Bytes";
import { useT } from "../i18n";

export function Activity() {
  const { t } = useT();
  const [entries, setEntries] = useState<JournalEntry[]>([]);
  const [message, setMessage] = useState<string | null>(null);

  async function refresh() {
    setEntries(await api.journal(200));
  }

  useEffect(() => { refresh(); }, []);

  async function onUndo(op_id: string) {
    setMessage(null);
    try {
      await api.undo(op_id);
      setMessage("Undone.");
    } catch (e) {
      setMessage(`Undo blocked: ${String(e)}`);
    }
    await refresh();
  }

  return (
    <div className="space-y-4">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t("activity.title")}</h1>
          <p className="text-sm text-text-muted">{t("activity.subtitle")}</p>
        </div>
        <button className="btn" onClick={refresh}>{t("activity.refresh")}</button>
      </header>

      {message && (
        <div className="rounded-md border border-border bg-surface-muted px-3 py-2 text-xs text-text-muted">
          {message}
        </div>
      )}

      {entries.length === 0 ? (
        <div className="card p-8 text-center text-sm text-text-faint">
          {t("activity.empty")}
        </div>
      ) : (
        <ul className="card divide-y divide-border">
          {entries.map((e) => (
            <li key={e.op_id} className="p-3">
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className="mb-0.5 flex items-center gap-2">
                    <span className="pill">{e.kind}</span>
                    <span className={stateClass(e.state)}>{e.state}</span>
                    <span className="text-xs text-text-faint">
                      {new Date(e.started_at).toLocaleString()} · {e.rule_name}
                    </span>
                  </div>
                  <div className="truncate font-mono text-sm">
                    {e.source ?? "(no source)"} → {e.destination ?? "(no destination)"}
                  </div>
                  <div className="text-xs text-text-faint">
                    {humanBytes(e.size)}
                    {e.error && (
                      <span className="ml-2 text-danger">error: {e.error}</span>
                    )}
                  </div>
                </div>
                <div className="flex flex-col items-end gap-1">
                  {e.undo_status === "available" && (
                    <button className="btn" onClick={() => onUndo(e.op_id)}>
                      {t("activity.undo")}
                    </button>
                  )}
                  {e.undo_status === "unsafe" && (
                    <span className="pill-warn">{t("activity.undo_unsafe")}</span>
                  )}
                  {e.undo_status === "done" && (
                    <span className="pill-ok">{t("activity.undone")}</span>
                  )}
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function stateClass(state: JournalEntry["state"]): string {
  switch (state) {
    case "completed": return "pill-ok";
    case "failed": return "pill-danger";
    case "undone": return "pill";
    default: return "pill-warn";
  }
}
