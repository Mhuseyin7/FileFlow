import { useState } from "react";
import { api } from "../api";
import { useT } from "../i18n";
import type { Rule } from "../types";

const TEMPLATES: Rule[] = [
  {
    id: "",
    name: "Organize screenshots by year",
    description:
      "Move old screenshots into ~/Archive/Screenshots/{year} once they are 30 days old.",
    priority: 100,
    enabled: false,
    profile: null,
    condition: {
      kind: "all",
      children: [
        {
          kind: "leaf",
          condition: { type: "extension", any_of: ["png", "jpg", "jpeg"] },
        },
        { kind: "leaf", condition: { type: "age_days", op: "gt", value: 30 } },
        { kind: "leaf", condition: { type: "filename_contains", needle: "screenshot" } },
      ],
    },
    action: {
      kind: "move",
      dest_dir: "~/Archive/Screenshots",
      rename: "{year}/{name}",
    },
    on_conflict: "rename",
    then: "stop_after_action",
    sources: [],
    trigger: "weekly",
  },
  {
    id: "",
    name: "Archive old installers",
    description: "Move .exe/.msi/.dmg older than 30 days into Installers/Archive.",
    priority: 110,
    enabled: false,
    profile: null,
    condition: {
      kind: "all",
      children: [
        {
          kind: "leaf",
          condition: { type: "extension", any_of: ["exe", "msi", "dmg", "pkg", "deb"] },
        },
        { kind: "leaf", condition: { type: "age_days", op: "gt", value: 30 } },
      ],
    },
    action: {
      kind: "move",
      dest_dir: "~/Documents/Installers/Archive",
      rename: null,
    },
    on_conflict: "rename",
    then: "stop_after_action",
    sources: [],
    trigger: "weekly",
  },
  {
    id: "",
    name: "Downloads → PDF library",
    description: "Move Downloads/*.pdf into Documents/PDF.",
    priority: 120,
    enabled: false,
    profile: null,
    condition: {
      kind: "all",
      children: [
        { kind: "leaf", condition: { type: "extension", any_of: ["pdf"] } },
      ],
    },
    action: {
      kind: "move",
      dest_dir: "~/Documents/PDF",
      rename: null,
    },
    on_conflict: "rename",
    then: "stop_after_action",
    sources: ["~/Downloads"],
    trigger: "real_time",
  },
  {
    id: "",
    name: "Separate archives",
    description: "Move .zip/.7z/.tar.gz into Archives.",
    priority: 130,
    enabled: false,
    profile: null,
    condition: {
      kind: "all",
      children: [
        {
          kind: "leaf",
          condition: {
            type: "extension",
            any_of: ["zip", "7z", "tar", "gz", "tgz", "rar", "xz"],
          },
        },
      ],
    },
    action: { kind: "move", dest_dir: "~/Archives", rename: null },
    on_conflict: "rename",
    then: "stop_after_action",
    sources: ["~/Downloads"],
    trigger: "real_time",
  },
  {
    id: "",
    name: "Review huge downloads",
    description: "Any file over 5 GB goes to the review queue.",
    priority: 90,
    enabled: false,
    profile: null,
    condition: {
      kind: "all",
      children: [
        {
          kind: "leaf",
          condition: { type: "size_bytes", op: "gt", value: 5 * 1024 ** 3 },
        },
      ],
    },
    action: { kind: "review" },
    on_conflict: "rename",
    then: "stop_after_action",
    sources: ["~/Downloads"],
    trigger: "real_time",
  },
];

export function Templates() {
  const { t } = useT();
  const [message, setMessage] = useState<string | null>(null);

  async function install(tpl: Rule) {
    await api.saveRule({ ...tpl, id: "" });
    setMessage(`"${tpl.name}" — ok.`);
  }

  return (
    <div className="space-y-4">
      <header>
        <h1 className="text-2xl font-semibold">{t("templates.title")}</h1>
        <p className="text-sm text-text-muted">{t("templates.subtitle")}</p>
      </header>

      {message && (
        <div className="rounded-md border border-ok bg-ok-soft px-3 py-2 text-xs text-ok">
          {message}
        </div>
      )}

      <ul className="grid grid-cols-1 gap-3 md:grid-cols-2">
        {TEMPLATES.map((tpl) => (
          <li key={tpl.name} className="card p-4">
            <div className="mb-1 flex items-center justify-between">
              <div className="text-sm font-semibold">{tpl.name}</div>
              <span className="pill">{tpl.trigger.replace("_", " ")}</span>
            </div>
            <p className="mb-3 text-xs text-text-muted">{tpl.description}</p>
            <button className="btn-primary" onClick={() => install(tpl)}>
              {t("templates.install")}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
