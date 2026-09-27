import { useEffect, useState } from "react";
import { api } from "../api";
import { useT } from "../i18n";

export function Folders() {
  const { t } = useT();
  const [roots, setRoots] = useState<string[]>([]);
  const [draft, setDraft] = useState("");

  async function refresh() {
    setRoots(await api.getSandboxRoots());
  }
  useEffect(() => { refresh(); }, []);

  async function add() {
    const path = draft.trim();
    if (!path) return;
    const next = Array.from(new Set([...roots, path]));
    await api.setSandboxRoots(next);
    setDraft("");
    await refresh();
  }

  async function remove(p: string) {
    await api.setSandboxRoots(roots.filter((r) => r !== p));
    await refresh();
  }

  return (
    <div className="space-y-4">
      <header>
        <h1 className="text-2xl font-semibold">{t("folders.title")}</h1>
        <p className="text-sm text-text-muted">{t("folders.subtitle")}</p>
      </header>

      <div className="card p-4">
        <div className="flex items-center gap-2">
          <input
            className="flex-1 rounded-md border border-border bg-surface-muted px-3 py-1.5 text-sm"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="C:\Users\you\Downloads"
            onKeyDown={(e) => {
              if (e.key === "Enter") add();
            }}
          />
          <button className="btn-primary" onClick={add}>
            {t("folders.add")}
          </button>
        </div>
      </div>

      <ul className="card divide-y divide-border">
        {roots.length === 0 && (
          <li className="p-4 text-sm text-text-faint">{t("folders.empty")}</li>
        )}
        {roots.map((r) => (
          <li key={r} className="flex items-center justify-between p-3">
            <code className="text-sm text-text">{r}</code>
            <button className="btn-danger" onClick={() => remove(r)}>
              {t("folders.revoke")}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
