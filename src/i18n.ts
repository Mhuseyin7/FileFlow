// Minimal i18n system. No runtime dependency — one dict per language, a
// `useT()` hook that pulls the current language from React state, and a
// small `setLang` helper that persists the choice to localStorage.

import { createContext, createElement, useContext, useEffect, useState } from "react";
import type { PropsWithChildren } from "react";

export type Lang = "en" | "tr";

const dict = {
  en: {
    // Nav
    "nav.dashboard": "Dashboard",
    "nav.dashboard.hint": "Overview",
    "nav.rules": "Rules",
    "nav.rules.hint": "Conditions & actions",
    "nav.activity": "Activity",
    "nav.activity.hint": "History + undo",
    "nav.review": "Review",
    "nav.review.hint": "Files awaiting approval",
    "nav.folders": "Folders",
    "nav.folders.hint": "Monitored roots",
    "nav.templates": "Templates",
    "nav.templates.hint": "Safe examples",
    "nav.settings": "Settings",
    "nav.settings.hint": "Privacy & advanced",
    "app.title": "FileFlow",
    "app.tagline": "Explainable file organizer",
    "app.dev_banner":
      "Browser preview mode — using demo data. Launch via `npm run tauri dev` for the real app.",

    // Dashboard
    "dashboard.title": "Today",
    "dashboard.subtitle":
      "FileFlow only reports moves it actually made. Moving files does not save disk space — that number is intentionally absent.",
    "dashboard.organized": "Files organized",
    "dashboard.bytes": "Bytes moved",
    "dashboard.review": "Awaiting review",
    "dashboard.active_rules": "Active rules",
    "dashboard.manage": "Manage rules →",
    "dashboard.recent": "Recent activity",
    "dashboard.view_all": "View all →",
    "dashboard.empty":
      "No activity yet. Start by creating a rule and running a preview.",

    // Rules
    "rules.title": "Rules",
    "rules.new": "+ New rule",
    "rules.empty": "No rules yet. Create one to get started.",
    "rules.select": "Select or create a rule.",
    "rules.on": "on",
    "rules.off": "off",
    "rules.preview": "Preview",
    "rules.previewing": "Previewing…",
    "rules.save": "Save",
    "rules.delete": "Delete",
    "rules.enable": "Enable this rule",
    "rules.dry_run": "Dry run",
    "rules.run_once": "Run once",

    // Activity
    "activity.title": "Activity",
    "activity.subtitle":
      "Every operation FileFlow has recorded. Undo is only offered when we can verify the file at the destination hasn't been modified.",
    "activity.refresh": "Refresh",
    "activity.empty": "No activity yet.",
    "activity.undo": "Undo",
    "activity.undone": "undone",
    "activity.undo_unsafe": "undo unsafe",

    // Review
    "review.title": "Review queue",
    "review.subtitle":
      "Files a rule flagged as uncertain — FileFlow won't move them without your approval.",
    "review.empty": "No files awaiting review.",
    "review.hint":
      'Add a rule with the "Send to review" action to see items here.',

    // Folders
    "folders.title": "Monitored folders",
    "folders.subtitle":
      "FileFlow only touches folders you explicitly grant. It never crawls your whole computer.",
    "folders.add": "Grant access",
    "folders.empty": "No folders granted yet.",
    "folders.revoke": "Revoke",

    // Templates
    "templates.title": "Templates",
    "templates.subtitle":
      "Safe starting points. Every template installs as disabled — you review the fit, preview, and only then enable.",
    "templates.install": "Add (disabled)",

    // Settings
    "settings.title": "Settings",
    "settings.subtitle":
      "FileFlow is local-first. There is no account, no cloud, and no telemetry.",
    "settings.privacy": "Privacy",
    "settings.safety": "Safety defaults",
    "settings.data": "Data",
    "settings.language": "Language",
    "settings.language_en": "English",
    "settings.language_tr": "Türkçe",
  },
  tr: {
    // Nav
    "nav.dashboard": "Panel",
    "nav.dashboard.hint": "Genel bakış",
    "nav.rules": "Kurallar",
    "nav.rules.hint": "Koşullar ve eylemler",
    "nav.activity": "Etkinlik",
    "nav.activity.hint": "Geçmiş + geri al",
    "nav.review": "İnceleme",
    "nav.review.hint": "Onay bekleyen dosyalar",
    "nav.folders": "Klasörler",
    "nav.folders.hint": "İzlenen kökler",
    "nav.templates": "Şablonlar",
    "nav.templates.hint": "Güvenli örnekler",
    "nav.settings": "Ayarlar",
    "nav.settings.hint": "Gizlilik ve gelişmiş",
    "app.title": "FileFlow",
    "app.tagline": "Açıklanabilir dosya düzenleyici",
    "app.dev_banner":
      "Tarayıcı önizleme modu — demo veri kullanılıyor. Gerçek uygulama için `npm run tauri dev`.",

    // Dashboard
    "dashboard.title": "Bugün",
    "dashboard.subtitle":
      "FileFlow yalnızca gerçekten yaptığı taşımaları raporlar. Dosyaları taşımak disk alanı kazandırmaz — bu sayı bilinçli olarak yok.",
    "dashboard.organized": "Düzenlenen dosya",
    "dashboard.bytes": "Taşınan bayt",
    "dashboard.review": "İnceleme bekleyen",
    "dashboard.active_rules": "Aktif kural",
    "dashboard.manage": "Kuralları yönet →",
    "dashboard.recent": "Son etkinlik",
    "dashboard.view_all": "Tümünü gör →",
    "dashboard.empty":
      "Henüz etkinlik yok. Bir kural oluşturup önizleme çalıştırarak başlayın.",

    // Rules
    "rules.title": "Kurallar",
    "rules.new": "+ Yeni kural",
    "rules.empty": "Henüz kural yok. Başlamak için bir tane oluşturun.",
    "rules.select": "Bir kural seçin veya oluşturun.",
    "rules.on": "açık",
    "rules.off": "kapalı",
    "rules.preview": "Önizle",
    "rules.previewing": "Önizleniyor…",
    "rules.save": "Kaydet",
    "rules.delete": "Sil",
    "rules.enable": "Bu kuralı etkinleştir",
    "rules.dry_run": "Kuru çalıştır",
    "rules.run_once": "Bir kez çalıştır",

    // Activity
    "activity.title": "Etkinlik",
    "activity.subtitle":
      "FileFlow'un kaydettiği her işlem. Geri al, yalnızca hedefteki dosyanın değişmediği doğrulandığında sunulur.",
    "activity.refresh": "Yenile",
    "activity.empty": "Henüz etkinlik yok.",
    "activity.undo": "Geri al",
    "activity.undone": "geri alındı",
    "activity.undo_unsafe": "geri alma güvensiz",

    // Review
    "review.title": "İnceleme kuyruğu",
    "review.subtitle":
      "Bir kuralın belirsiz olarak işaretlediği dosyalar — FileFlow onayınız olmadan taşımaz.",
    "review.empty": "İnceleme bekleyen dosya yok.",
    "review.hint":
      'Burada öğe görmek için "İncelemeye gönder" eylemi olan bir kural ekleyin.',

    // Folders
    "folders.title": "İzlenen klasörler",
    "folders.subtitle":
      "FileFlow yalnızca sizin açıkça izin verdiğiniz klasörlere dokunur. Tüm bilgisayarınızı asla taramaz.",
    "folders.add": "Erişim ver",
    "folders.empty": "Henüz klasör izni verilmedi.",
    "folders.revoke": "Kaldır",

    // Templates
    "templates.title": "Şablonlar",
    "templates.subtitle":
      "Güvenli başlangıç noktaları. Her şablon devre dışı yüklenir — önce inceleyip önizleyin, sonra etkinleştirin.",
    "templates.install": "Ekle (devre dışı)",

    // Settings
    "settings.title": "Ayarlar",
    "settings.subtitle":
      "FileFlow yerel önceliklidir. Hesap, bulut ve telemetri yoktur.",
    "settings.privacy": "Gizlilik",
    "settings.safety": "Güvenlik varsayılanları",
    "settings.data": "Veri",
    "settings.language": "Dil",
    "settings.language_en": "English",
    "settings.language_tr": "Türkçe",
  },
} satisfies Record<Lang, Record<string, string>>;

type Key = keyof (typeof dict)["en"];

const I18nContext = createContext<{
  lang: Lang;
  setLang: (l: Lang) => void;
  t: (k: Key) => string;
}>({
  lang: "en",
  setLang: () => {},
  t: (k) => dict.en[k],
});

const STORAGE_KEY = "fileflow.lang";

export function I18nProvider(props: PropsWithChildren) {
  const [lang, setLangState] = useState<Lang>(() => {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw === "en" || raw === "tr") return raw;
    } catch {
      /* localStorage may be unavailable */
    }
    const nav = typeof navigator !== "undefined" ? navigator.language : "en";
    return nav.toLowerCase().startsWith("tr") ? "tr" : "en";
  });

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, lang);
    } catch {
      /* ignore */
    }
    if (typeof document !== "undefined") {
      document.documentElement.lang = lang;
    }
  }, [lang]);

  const value = {
    lang,
    setLang: setLangState,
    t: (k: Key) => dict[lang][k] ?? dict.en[k],
  };
  return createElement(I18nContext.Provider, { value }, props.children);
}

export function useT() { return useContext(I18nContext); }
