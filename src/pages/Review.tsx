import { useT } from "../i18n";

export function Review() {
  const { t } = useT();
  return (
    <div className="space-y-4">
      <header>
        <h1 className="text-2xl font-semibold">{t("review.title")}</h1>
        <p className="text-sm text-text-muted">{t("review.subtitle")}</p>
      </header>
      <div className="card p-8 text-center text-sm text-text-faint">
        {t("review.empty")}
        <div className="mt-2 text-xs">{t("review.hint")}</div>
      </div>
    </div>
  );
}
