import { useEffect, useState } from "react";
import { api, errorText, pickFolder } from "../api";
import { useT } from "../i18n";

export function Folders() {
  const { t } = useT();
  const [roots, setRoots] = useState<string[]>([]);
  const [draft, setDraft] = useState("");
  const [message, setMessage] = useState<string | null>(null);

  async function refresh() {
    setRoots(await api.getSandboxRoots());
  }
  useEffect(() => {
    refresh().catch((e) => setMessage(errorText(e)));
  }, []);

  async function grant(path: string) {
    const p = path.trim();
    if (!p) return;
    setMessage(null);
    try {
      await api.setSandboxRoots(Array.from(new Set([...roots, p])));
      setDraft("");
    } catch (e) {
      // The backend refuses drive roots, system folders and missing paths.
      setMessage(errorText(e));
    }
    await refresh();
  }

  async function browse() {
    const picked = await pickFolder();
    if (picked) await grant(picked);
  }

  async function remove(p: string) {
    setMessage(null);
    try {
      await api.setSandboxRoots(roots.filter((r) => r !== p));
    } catch (e) {
      setMessage(errorText(e));
    }
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
              if (e.key === "Enter") grant(draft);
            }}
          />
          <button className="btn" onClick={browse}>
            {t("folders.browse")}
          </button>
          <button className="btn-primary" onClick={() => grant(draft)}>
            {t("folders.add")}
          </button>
        </div>
        {message && (
          <div className="mt-3 rounded-md border border-danger bg-danger-soft px-3 py-2 text-xs text-danger">
            {message}
          </div>
        )}
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
