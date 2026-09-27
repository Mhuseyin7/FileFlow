import { useT } from "../i18n";

export type PageKey =
  | "dashboard"
  | "rules"
  | "activity"
  | "review"
  | "folders"
  | "templates"
  | "settings";

export function Nav({
  active,
  onNavigate,
}: {
  active: PageKey;
  onNavigate: (k: PageKey) => void;
}) {
  const { t } = useT();
  const items: { key: PageKey; label: string; hint: string }[] = [
    { key: "dashboard", label: t("nav.dashboard"), hint: t("nav.dashboard.hint") },
    { key: "rules", label: t("nav.rules"), hint: t("nav.rules.hint") },
    { key: "activity", label: t("nav.activity"), hint: t("nav.activity.hint") },
    { key: "review", label: t("nav.review"), hint: t("nav.review.hint") },
    { key: "folders", label: t("nav.folders"), hint: t("nav.folders.hint") },
    { key: "templates", label: t("nav.templates"), hint: t("nav.templates.hint") },
    { key: "settings", label: t("nav.settings"), hint: t("nav.settings.hint") },
  ];
  return (
    <nav className="flex w-56 shrink-0 flex-col border-r border-border bg-surface-raised">
      <div className="px-4 py-4">
        <div className="text-base font-semibold tracking-tight">
          {t("app.title")}
        </div>
        <div className="text-xs text-text-faint">{t("app.tagline")}</div>
      </div>
      <ul className="flex-1 space-y-0.5 px-2">
        {items.map((it) => (
          <li key={it.key}>
            <button
              onClick={() => onNavigate(it.key)}
              className={`w-full rounded-md px-3 py-2 text-left transition ${
                active === it.key
                  ? "bg-accent-soft text-accent"
                  : "text-text-muted hover:bg-surface-muted hover:text-text"
              }`}
            >
              <div className="text-sm font-medium">{it.label}</div>
              <div className="text-[11px] text-text-faint">{it.hint}</div>
            </button>
          </li>
        ))}
      </ul>
      <div className="border-t border-border px-4 py-3 text-[11px] text-text-faint">
        v0.1.0 · local-first
      </div>
    </nav>
  );
}
