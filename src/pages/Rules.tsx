import { useEffect, useMemo, useState } from "react";
import { api, errorText, onEngineChange } from "../api";
import type {
  Category,
  Condition,
  ConditionNode,
  NumOp,
  PreviewResult,
  Rule,
} from "../types";
import { humanBytes } from "../components/Bytes";
import { useT } from "../i18n";

export function Rules() {
  const { t } = useT();
  const [rules, setRules] = useState<Rule[]>([]);
  const [selected, setSelected] = useState<Rule | null>(null);
  const [preview, setPreview] = useState<PreviewResult | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [running, setRunning] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function refresh() {
    const rs = await api.listRules();
    setRules(rs);
    if (selected) {
      const updated = rs.find((r) => r.id === selected.id);
      setSelected(updated ?? null);
    }
  }

  useEffect(() => {
    refresh();
    let off = () => {};
    onEngineChange(refresh).then((fn) => (off = fn));
    return () => off();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function onNew() {
    const draft: Rule = {
      id: "",
      name: "Untitled rule",
      description: "",
      priority: 100,
      enabled: false,
      profile: null,
      condition: {
        kind: "all",
        children: [
          { kind: "leaf", condition: { type: "extension", any_of: ["pdf"] } },
        ],
      },
      action: { kind: "move", dest_dir: "", rename: null },
      on_conflict: "rename",
      then: "stop_after_action",
      sources: [],
      trigger: "real_time",
    };
    // Not stored until it passes validation (Save).
    setSelected(draft);
    setPreview(null);
    setMessage("Fill in the destination folder, then press Save.");
  }

  async function onSave(r: Rule) {
    try {
      const saved = await api.saveRule(r);
      setSelected(saved);
      await refresh();
      setMessage("Saved.");
    } catch (e) {
      setMessage(`Not saved: ${errorText(e)}`);
    }
  }

  async function onDelete(id: string) {
    try {
      if (id) await api.deleteRule(id);
      setSelected(null);
      setPreview(null);
      await refresh();
    } catch (e) {
      setMessage(`Delete failed: ${errorText(e)}`);
    }
  }

  async function onPreview() {
    if (!selected) return;
    setPreviewing(true);
    setPreview(null);
    setMessage(null);
    try {
      if (!selected.id) {
        setMessage("Save the rule first, then preview it.");
        return;
      }
      const root = firstSource(selected) ?? (await api.getSandboxRoots())[0];
      if (!root) {
        setMessage("Grant access to a folder first (Folders page).");
        return;
      }
      setPreview(await api.preview(root, selected.id));
    } catch (e) {
      setMessage(`Preview failed: ${errorText(e)}`);
    } finally {
      setPreviewing(false);
    }
  }

  async function onRun(dryRun: boolean) {
    if (!preview) return;
    setRunning(true);
    try {
      const outcomes = await api.run(preview.ops, dryRun);
      const count = (s: string) => outcomes.filter((o) => o.status === s).length;
      const parts = [
        `${count("done")} ${dryRun ? "would apply" : "done"}`,
        count("skipped") ? `${count("skipped")} skipped` : "",
        count("needs_review") ? `${count("needs_review")} sent to review` : "",
        count("failed") ? `${count("failed")} failed` : "",
      ].filter(Boolean);
      const firstError = outcomes.find((o) => o.status === "failed")?.error;
      setMessage(
        `${dryRun ? "Dry run" : "Run"}: ${parts.join(", ")}.` +
          (firstError ? ` First error: ${firstError}` : ""),
      );
      if (!dryRun) setPreview(null);
    } catch (e) {
      setMessage(`Run failed: ${errorText(e)}`);
    } finally {
      setRunning(false);
    }
  }

  return (
    <div className="grid grid-cols-12 gap-6">
      <aside className="col-span-4 space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="text-lg font-semibold">{t("rules.title")}</h2>
          <button className="btn-primary" onClick={onNew}>
            {t("rules.new")}
          </button>
        </div>
        <ul className="card divide-y divide-border">
          {rules.length === 0 && (
            <li className="p-4 text-sm text-text-faint">
              {t("rules.empty")}
            </li>
          )}
          {rules.map((r) => (
            <li key={r.id}>
              <button
                onClick={() => {
                  setSelected(r);
                  setPreview(null);
                }}
                className={`flex w-full items-center justify-between px-3 py-2 text-left hover:bg-surface-muted ${
                  selected?.id === r.id ? "bg-surface-muted" : ""
                }`}
              >
                <div>
                  <div className="text-sm">{r.name}</div>
                  <div className="text-xs text-text-faint">
                    priority {r.priority} · {r.then.replace("_", " ")}
                  </div>
                </div>
                <span className={r.enabled ? "pill-ok" : "pill"}>
                  {r.enabled ? t("rules.on") : t("rules.off")}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </aside>

      <section className="col-span-8">
        {!selected ? (
          <div className="card flex h-64 items-center justify-center text-sm text-text-faint">
            {t("rules.select")}
          </div>
        ) : (
          <RuleEditor
            rule={selected}
            onChange={setSelected}
            onSave={onSave}
            onDelete={onDelete}
            onPreview={onPreview}
            previewing={previewing}
          />
        )}
        {preview && selected && (
          <PreviewPanel
            preview={preview}
            running={running}
            onRun={onRun}
          />
        )}
        {message && (
          <div className="mt-3 rounded-md border border-border bg-surface-muted px-3 py-2 text-xs text-text-muted">
            {message}
          </div>
        )}
      </section>
    </div>
  );
}

function firstSource(rule: Rule): string | null {
  if (rule.sources[0]) return rule.sources[0];
  const s = findSourceInCondition(rule.condition);
  return s;
}

function findSourceInCondition(
  node: ConditionNode,
): string | null {
  if (node.kind === "leaf") {
    return node.condition.type === "source_folder" ? node.condition.path : null;
  }
  for (const c of node.children) {
    const s = findSourceInCondition(c);
    if (s) return s;
  }
  return null;
}

// ---------------------------------------------------------------------------
// Rule editor: intentionally schema-first — the condition tree renders as a
// list of leaves for v1. Nested groups are supported by the engine and can be
// exposed once we have real user testing.
// ---------------------------------------------------------------------------
function RuleEditor({
  rule,
  onChange,
  onSave,
  onDelete,
  onPreview,
  previewing,
}: {
  rule: Rule;
  onChange: (r: Rule) => void;
  onSave: (r: Rule) => void;
  onDelete: (id: string) => void;
  onPreview: () => void;
  previewing: boolean;
}) {
  const leaves = useMemo(() => flattenLeaves(rule.condition), [rule.condition]);

  return (
    <div className="card space-y-4 p-5">
      <div className="grid grid-cols-2 gap-3">
        <div className="field">
          <label>Name</label>
          <input
            value={rule.name}
            onChange={(e) => onChange({ ...rule, name: e.target.value })}
          />
        </div>
        <div className="field">
          <label>Profile</label>
          <input
            value={rule.profile ?? ""}
            placeholder="Personal, Work, …"
            onChange={(e) =>
              onChange({ ...rule, profile: e.target.value || null })
            }
          />
        </div>
        <div className="field">
          <label>Priority</label>
          <input
            type="number"
            value={rule.priority}
            onChange={(e) =>
              onChange({ ...rule, priority: Number(e.target.value) })
            }
          />
        </div>
        <div className="field">
          <label>After this rule matches</label>
          <select
            value={rule.then}
            onChange={(e) =>
              onChange({
                ...rule,
                then: e.target.value as Rule["then"],
              })
            }
          >
            <option value="stop_after_action">Stop</option>
            <option value="continue">Continue to next rule</option>
          </select>
        </div>
      </div>

      <div>
        <div className="mb-2 text-xs uppercase tracking-wide text-text-faint">
          Conditions (ALL of)
        </div>
        <div className="space-y-2">
          {leaves.map((leaf, idx) => (
            <ConditionRow
              key={idx}
              leaf={leaf}
              onChange={(next) => {
                const nextLeaves = leaves.slice();
                nextLeaves[idx] = next;
                onChange({ ...rule, condition: buildAll(nextLeaves) });
              }}
              onDelete={() => {
                const nextLeaves = leaves.filter((_, i) => i !== idx);
                onChange({ ...rule, condition: buildAll(nextLeaves) });
              }}
            />
          ))}
          <button
            className="btn-ghost"
            onClick={() =>
              onChange({
                ...rule,
                condition: buildAll([
                  ...leaves,
                  {
                    kind: "leaf",
                    condition: { type: "extension", any_of: ["pdf"] },
                  },
                ]),
              })
            }
          >
            + Add condition
          </button>
        </div>
      </div>

      <ActionEditor
        rule={rule}
        onChange={(a) => onChange({ ...rule, action: a })}
      />

      <div className="grid grid-cols-2 gap-3">
        <div className="field">
          <label>On conflict</label>
          <select
            value={rule.on_conflict}
            onChange={(e) =>
              onChange({
                ...rule,
                on_conflict: e.target.value as Rule["on_conflict"],
              })
            }
          >
            <option value="rename">Rename (foo → foo (1))</option>
            <option value="ask">Ask</option>
            <option value="skip">Skip</option>
            <option value="replace">Replace</option>
          </select>
        </div>
        <div className="field">
          <label>Trigger</label>
          <select
            value={rule.trigger}
            onChange={(e) =>
              onChange({ ...rule, trigger: e.target.value as Rule["trigger"] })
            }
          >
            <option value="real_time">Real time (watcher)</option>
            <option value="manual">Manual</option>
            <option value="daily">Daily</option>
            <option value="weekly">Weekly</option>
          </select>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={rule.enabled}
            onChange={(e) =>
              onChange({ ...rule, enabled: e.target.checked })
            }
          />
          Enable this rule
        </label>
        <div className="flex-1" />
        <button
          className="btn"
          onClick={onPreview}
          disabled={previewing}
        >
          {previewing ? "Previewing…" : "Preview"}
        </button>
        <button className="btn-primary" onClick={() => onSave(rule)}>
          Save
        </button>
        <button
          className="btn-danger"
          onClick={() => onDelete(rule.id)}
        >
          Delete
        </button>
      </div>
    </div>
  );
}

function ConditionRow({
  leaf,
  onChange,
  onDelete,
}: {
  leaf: ConditionNode & { kind: "leaf" };
  onChange: (n: ConditionNode & { kind: "leaf" }) => void;
  onDelete: () => void;
}) {
  const c = leaf.condition;
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-surface-muted px-3 py-2">
      <select
        className="rounded-md border border-border bg-surface-raised px-2 py-1 text-sm"
        value={c.type}
        onChange={(e) => onChange({ kind: "leaf", condition: defaultForType(e.target.value as typeof c.type) })}
      >
        <option value="extension">Extension</option>
        <option value="filename_contains">Filename contains</option>
        <option value="filename_regex">Filename regex</option>
        <option value="source_folder">Source folder</option>
        <option value="category">Category</option>
        <option value="size_bytes">Size (bytes)</option>
        <option value="age_days">Age (days)</option>
        <option value="hidden">Hidden</option>
      </select>
      <div className="flex-1">
        {c.type === "extension" && (
          <input
            className="w-full rounded-md border border-border bg-surface-raised px-2 py-1 text-sm"
            value={c.any_of.join(", ")}
            onChange={(e) =>
              onChange({
                kind: "leaf",
                condition: {
                  type: "extension",
                  any_of: e.target.value.split(",").map((s) => s.trim()).filter(Boolean),
                },
              })
            }
            placeholder="pdf, docx"
          />
        )}
        {c.type === "filename_contains" && (
          <input
            className="w-full rounded-md border border-border bg-surface-raised px-2 py-1 text-sm"
            value={c.needle}
            onChange={(e) =>
              onChange({ kind: "leaf", condition: { type: "filename_contains", needle: e.target.value } })
            }
            placeholder="invoice"
          />
        )}
        {c.type === "filename_regex" && (
          <input
            className="w-full rounded-md border border-border bg-surface-raised px-2 py-1 text-sm font-mono"
            value={c.pattern}
            onChange={(e) =>
              onChange({ kind: "leaf", condition: { type: "filename_regex", pattern: e.target.value } })
            }
            placeholder="^invoice-\\d+\\.pdf$"
          />
        )}
        {c.type === "source_folder" && (
          <div className="flex items-center gap-2">
            <input
              className="flex-1 rounded-md border border-border bg-surface-raised px-2 py-1 text-sm"
              value={c.path}
              onChange={(e) =>
                onChange({
                  kind: "leaf",
                  condition: { type: "source_folder", path: e.target.value, recursive: c.recursive },
                })
              }
              placeholder="~/Downloads"
            />
            <label className="flex items-center gap-1 text-xs text-text-muted">
              <input
                type="checkbox"
                checked={c.recursive}
                onChange={(e) =>
                  onChange({
                    kind: "leaf",
                    condition: { type: "source_folder", path: c.path, recursive: e.target.checked },
                  })
                }
              />
              recursive
            </label>
          </div>
        )}
        {c.type === "category" && (
          <input
            className="w-full rounded-md border border-border bg-surface-raised px-2 py-1 text-sm"
            value={c.any_of.join(", ")}
            onChange={(e) =>
              onChange({
                kind: "leaf",
                condition: {
                  type: "category",
                  any_of: e.target.value
                    .split(",")
                    .map((s) => s.trim())
                    .filter(Boolean) as Category[],
                },
              })
            }
            placeholder="documents, images"
          />
        )}
        {(c.type === "size_bytes" || c.type === "age_days") && (
          <div className="flex items-center gap-2">
            <select
              className="rounded-md border border-border bg-surface-raised px-2 py-1 text-sm"
              value={c.op}
              onChange={(e) =>
                onChange({
                  kind: "leaf",
                  condition: { ...c, op: e.target.value as NumOp },
                })
              }
            >
              <option value="lt">&lt;</option>
              <option value="le">≤</option>
              <option value="eq">=</option>
              <option value="ge">≥</option>
              <option value="gt">&gt;</option>
            </select>
            <input
              type="number"
              className="flex-1 rounded-md border border-border bg-surface-raised px-2 py-1 text-sm"
              value={c.value}
              onChange={(e) =>
                onChange({
                  kind: "leaf",
                  condition: { ...c, value: Number(e.target.value) },
                })
              }
            />
            <span className="text-xs text-text-faint">
              {c.type === "size_bytes" ? "bytes" : "days"}
            </span>
          </div>
        )}
        {c.type === "hidden" && (
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={c.equals}
              onChange={(e) =>
                onChange({ kind: "leaf", condition: { type: "hidden", equals: e.target.checked } })
              }
            />
            file is hidden
          </label>
        )}
      </div>
      <button className="btn-ghost text-danger" onClick={onDelete}>
        Remove
      </button>
    </div>
  );
}

function ActionEditor({
  rule,
  onChange,
}: {
  rule: Rule;
  onChange: (a: Rule["action"]) => void;
}) {
  const a = rule.action;
  return (
    <div className="rounded-md border border-border bg-surface-muted p-3">
      <div className="mb-2 text-xs uppercase tracking-wide text-text-faint">
        Action
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <select
          className="rounded-md border border-border bg-surface-raised px-2 py-1 text-sm"
          value={a.kind}
          onChange={(e) => onChange(defaultAction(e.target.value as Rule["action"]["kind"]))}
        >
          <option value="move">Move to…</option>
          <option value="copy">Copy to…</option>
          <option value="rename">Rename to…</option>
          <option value="create_folder">Create folder…</option>
          <option value="tag">Tag</option>
          <option value="review">Send to review</option>
          <option value="trash">Move to FileFlow trash</option>
        </select>
        {(a.kind === "move" || a.kind === "copy") && (
          <>
            <input
              className="flex-1 rounded-md border border-border bg-surface-raised px-2 py-1 text-sm"
              value={a.dest_dir}
              onChange={(e) =>
                onChange({ ...a, dest_dir: e.target.value })
              }
              placeholder="~/Documents/PDF"
            />
            <input
              className="w-56 rounded-md border border-border bg-surface-raised px-2 py-1 text-sm font-mono"
              value={a.rename ?? ""}
              onChange={(e) =>
                onChange({ ...a, rename: e.target.value || null })
              }
              placeholder="rename template (opt) e.g. {date}-{name}"
            />
          </>
        )}
        {a.kind === "rename" && (
          <input
            className="flex-1 rounded-md border border-border bg-surface-raised px-2 py-1 text-sm font-mono"
            value={a.pattern}
            onChange={(e) => onChange({ kind: "rename", pattern: e.target.value })}
            placeholder="{date}-{stem}.{ext}"
          />
        )}
        {a.kind === "create_folder" && (
          <input
            className="flex-1 rounded-md border border-border bg-surface-raised px-2 py-1 text-sm"
            value={a.path}
            onChange={(e) => onChange({ kind: "create_folder", path: e.target.value })}
            placeholder="~/Archive/{year}"
          />
        )}
        {a.kind === "tag" && (
          <input
            className="flex-1 rounded-md border border-border bg-surface-raised px-2 py-1 text-sm"
            value={a.tags.join(", ")}
            onChange={(e) =>
              onChange({
                kind: "tag",
                tags: e.target.value.split(",").map((s) => s.trim()).filter(Boolean),
              })
            }
            placeholder="invoices, 2026"
          />
        )}
      </div>
    </div>
  );
}

function PreviewPanel({
  preview,
  running,
  onRun,
}: {
  preview: PreviewResult;
  running: boolean;
  onRun: (dryRun: boolean) => void;
}) {
  return (
    <div className="card mt-4 p-4">
      <div className="mb-3 flex flex-wrap items-baseline justify-between gap-2">
        <div>
          <div className="text-sm text-text-muted">Preview</div>
          <div className="text-2xl font-semibold">
            {preview.summary.count} files ·{" "}
            <span className="text-text-muted">
              {humanBytes(preview.summary.total_bytes)}
            </span>
          </div>
        </div>
        <div className="flex gap-2">
          <button
            className="btn"
            onClick={() => onRun(true)}
            disabled={running || preview.summary.count === 0}
          >
            Dry run
          </button>
          <button
            className="btn-primary"
            onClick={() => onRun(false)}
            disabled={running || preview.summary.count === 0}
          >
            Run once
          </button>
        </div>
      </div>
      {preview.issues.length > 0 && (
        <div className="mb-3 rounded-md border border-warn bg-warn-soft p-3 text-xs text-warn">
          {preview.issues.length} file(s) matched but could not be planned:
          <ul className="mt-1 list-disc pl-4">
            {preview.issues.slice(0, 5).map((i, n) => (
              <li key={n}>
                {i.path}: {i.message}
              </li>
            ))}
          </ul>
        </div>
      )}
      {preview.errors.length > 0 && (
        <div className="mb-3 rounded-md border border-danger bg-danger-soft p-3 text-xs text-danger">
          {preview.errors.length} op(s) blocked by safety validator:
          <ul className="mt-1 list-disc pl-4">
            {preview.errors.slice(0, 5).map((e) => (
              <li key={e.op_id}>{e.message}</li>
            ))}
          </ul>
        </div>
      )}
      <ul className="max-h-72 divide-y divide-border overflow-auto">
        {preview.ops.slice(0, 200).map((op) => (
          <li key={op.id} className="py-2 text-sm">
            <div className="font-mono">
              {op.source ?? ""} → {op.destination ?? "(no destination)"}
            </div>
            <div className="text-xs text-text-faint">
              {op.reasons.join(" · ")}
              {op.warnings.length > 0 && (
                <span className="ml-2 text-warn">
                  ⚠ {op.warnings.join("; ")}
                </span>
              )}
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}

function flattenLeaves(
  node: ConditionNode,
): (ConditionNode & { kind: "leaf" })[] {
  if (node.kind === "leaf") return [node];
  return node.children.flatMap(flattenLeaves);
}

function buildAll(
  leaves: (ConditionNode & { kind: "leaf" })[],
): ConditionNode {
  return { kind: "all", children: leaves };
}

function defaultForType(t: Condition["type"]): Condition {
  switch (t) {
    case "extension": return { type: "extension", any_of: ["pdf"] };
    case "filename_contains": return { type: "filename_contains", needle: "" };
    case "filename_regex": return { type: "filename_regex", pattern: "" };
    case "source_folder": return { type: "source_folder", path: "", recursive: true };
    case "category": return { type: "category", any_of: ["documents"] };
    case "mime_type": return { type: "mime_type", any_of: ["application/pdf"] };
    case "size_bytes": return { type: "size_bytes", op: "gt", value: 0 };
    case "age_days": return { type: "age_days", op: "gt", value: 30 };
    case "hidden": return { type: "hidden", equals: false };
    case "modified_between": return { type: "modified_between", from: null, to: null };
    case "photo_capture_age_days": return { type: "photo_capture_age_days", op: "gt", value: 30 };
    case "pdf_page_count": return { type: "pdf_page_count", op: "gt", value: 10 };
  }
}

function defaultAction(kind: Rule["action"]["kind"]): Rule["action"] {
  switch (kind) {
    case "move": return { kind: "move", dest_dir: "", rename: null };
    case "copy": return { kind: "copy", dest_dir: "", rename: null };
    case "rename": return { kind: "rename", pattern: "{stem}.{ext}" };
    case "create_folder": return { kind: "create_folder", path: "" };
    case "tag": return { kind: "tag", tags: [] };
    case "review": return { kind: "review" };
    case "trash": return { kind: "trash" };
  }
}
