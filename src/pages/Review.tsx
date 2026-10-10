import { useEffect, useState } from "react";
import { api, errorText, onEngineChange } from "../api";
import type { ReviewItem } from "../types";
import { useT } from "../i18n";

export function Review() {
  const { t } = useT();
  const [items, setItems] = useState<ReviewItem[]>([]);
  const [message, setMessage] = useState<string | null>(null);

  async function refresh() {
    try {
      setItems(await api.reviewQueue());
    } catch (e) {
      setMessage(errorText(e));
    }
  }

  useEffect(() => {
    refresh();
    let off = () => {};
    onEngineChange(refresh).then((fn) => (off = fn));
    return () => off();
  }, []);

  async function dismiss(id: string) {
    try {
      await api.resolveReview(id, "dismissed");
    } catch (e) {
      setMessage(errorText(e));
    }
    await refresh();
  }

  return (
    <div className="space-y-4">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t("review.title")}</h1>
          <p className="text-sm text-text-muted">{t("review.subtitle")}</p>
        </div>
        <button className="btn" onClick={refresh}>
          {t("activity.refresh")}
        </button>
      </header>

      {message && (
        <div className="rounded-md border border-border bg-surface-muted px-3 py-2 text-xs text-text-muted">
          {message}
        </div>
      )}

      {items.length === 0 ? (
        <div className="card p-8 text-center text-sm text-text-faint">
          {t("review.empty")}
          <div className="mt-2 text-xs">{t("review.hint")}</div>
        </div>
      ) : (
        <ul className="card divide-y divide-border">
          {items.map((it) => (
            <li key={it.id} className="flex items-start justify-between gap-3 p-3">
              <div className="min-w-0 flex-1">
                <div className="truncate font-mono text-sm">{it.path}</div>
                <div className="text-xs text-text-faint">
                  {it.rule_name} · {it.reason} ·{" "}
                  {new Date(it.added_at * 1000).toLocaleString()}
                </div>
              </div>
              <button className="btn" onClick={() => dismiss(it.id)}>
                {t("review.dismiss")}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
