import { useT } from "../i18n";
import type { Lang } from "../i18n";

export function Settings() {
  const { t, lang, setLang } = useT();
  return (
    <div className="space-y-4">
      <header>
        <h1 className="text-2xl font-semibold">{t("settings.title")}</h1>
        <p className="text-sm text-text-muted">{t("settings.subtitle")}</p>
      </header>

      <section className="card p-4">
        <h2 className="mb-2 text-sm font-semibold">{t("settings.language")}</h2>
        <div className="flex gap-2">
          {(["en", "tr"] as Lang[]).map((code) => (
            <button
              key={code}
              onClick={() => setLang(code)}
              className={lang === code ? "btn-primary" : "btn"}
            >
              {code === "en"
                ? t("settings.language_en")
                : t("settings.language_tr")}
            </button>
          ))}
        </div>
      </section>

      <section className="card p-4">
        <h2 className="mb-2 text-sm font-semibold">{t("settings.privacy")}</h2>
        <ul className="space-y-1 text-sm text-text-muted">
          <li>• Filenames and paths never leave your machine.</li>
          <li>• No document contents are ever read, hashed, or transmitted for classification.</li>
          <li>
            • Checksums are computed locally for cross-filesystem moves and undo
            identity verification. They are never uploaded.
          </li>
          <li>• No account. No sign-in. No usage analytics.</li>
        </ul>
      </section>

      <section className="card p-4">
        <h2 className="mb-2 text-sm font-semibold">{t("settings.safety")}</h2>
        <ul className="space-y-1 text-sm text-text-muted">
          <li>• FileFlow never permanently deletes a file on its own.</li>
          <li>• Cross-filesystem moves verify a SHA-256 hash before the source is removed.</li>
          <li>• Undo refuses when the destination file has changed since the move.</li>
          <li>• Rules run against normalized metadata, never raw filesystem events.</li>
        </ul>
      </section>

      <section className="card p-4">
        <h2 className="mb-2 text-sm font-semibold">{t("settings.data")}</h2>
        <p className="text-sm text-text-muted">
          Rules, folders, and the action journal live in a local SQLite file
          under the platform&apos;s app-data directory. Delete the folder to
          reset FileFlow.
        </p>
      </section>
    </div>
  );
}
