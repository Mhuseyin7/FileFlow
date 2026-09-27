// Thin wrapper around Tauri's `invoke`. Falls back to a stub when the app is
// loaded outside Tauri (e.g. `vite dev` in a plain browser) so the UI stays
// interactable during development.

import type {
  JournalEntry,
  PlannedOp,
  PreviewResult,
  Rule,
  RunOutcome,
} from "./types";

type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

const tauri = (window as unknown as { __TAURI_INTERNALS__?: unknown })
  .__TAURI_INTERNALS__;

let invoke: Invoke;
if (tauri) {
  // Loaded lazily so the browser dev fallback below doesn't try to import it.
  const mod = await import("@tauri-apps/api/core");
  invoke = mod.invoke as Invoke;
} else {
  invoke = stubInvoke;
}

export const api = {
  listRules: () => invoke<Rule[]>("list_rules"),
  saveRule: (rule: Rule) => invoke<Rule>("save_rule", { rule }),
  deleteRule: (id: string) => invoke<void>("delete_rule", { id }),
  getSandboxRoots: () => invoke<string[]>("get_sandbox_roots"),
  setSandboxRoots: (roots: string[]) =>
    invoke<void>("set_sandbox_roots", { roots }),
  preview: (root: string) => invoke<PreviewResult>("preview", { root }),
  run: (ops: PlannedOp[], dryRun: boolean) =>
    invoke<RunOutcome[]>("run", { ops, dryRun }),
  journal: (limit: number) => invoke<JournalEntry[]>("journal", { limit }),
  undo: (opId: string) => invoke<void>("undo", { opId }),
};

// ---------------------------------------------------------------------------
// Browser dev stub. Returns plausible mock data so the UI can be styled and
// clicked around without spinning up the desktop shell. Every response ships
// with a `__stub__` marker so the UI can badge "demo data".
// ---------------------------------------------------------------------------
const stubRules: Rule[] = [
  {
    id: "demo-pdfs",
    name: "Organize PDFs",
    description: "Move Downloads/*.pdf into Documents/PDF",
    priority: 10,
    enabled: false,
    profile: "Personal",
    condition: {
      kind: "all",
      children: [
        { kind: "leaf", condition: { type: "extension", any_of: ["pdf"] } },
      ],
    },
    action: { kind: "move", dest_dir: "~/Documents/PDF", rename: null },
    on_conflict: "rename",
    then: "stop_after_action",
    sources: ["~/Downloads"],
    trigger: "real_time",
  },
];

let stubJournal: JournalEntry[] = [];

async function stubInvoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  switch (cmd) {
    case "list_rules":
      return stubRules as unknown as T;
    case "save_rule": {
      const r = args?.rule as Rule;
      const idx = stubRules.findIndex((x) => x.id === r.id);
      if (idx >= 0) stubRules[idx] = r;
      else stubRules.push({ ...r, id: r.id || crypto.randomUUID() });
      return r as unknown as T;
    }
    case "delete_rule": {
      const id = args?.id as string;
      const idx = stubRules.findIndex((r) => r.id === id);
      if (idx >= 0) stubRules.splice(idx, 1);
      return undefined as unknown as T;
    }
    case "get_sandbox_roots":
      return ["~/Downloads"] as unknown as T;
    case "set_sandbox_roots":
      return undefined as unknown as T;
    case "preview":
      return {
        ops: [
          {
            id: "demo-op-1",
            rule_id: "demo-pdfs",
            rule_name: "Organize PDFs",
            kind: "move",
            source: "~/Downloads/invoice-2026.pdf",
            destination: "~/Documents/PDF/invoice-2026.pdf",
            size: 384_120,
            collision: "rename",
            reasons: [
              "✓ extension in [pdf]",
              "✓ folder = ~/Downloads",
            ],
            warnings: [],
          },
        ],
        summary: { count: 1, total_bytes: 384_120, warnings: 0 },
        errors: [],
      } as unknown as T;
    case "run": {
      const ops = (args?.ops ?? []) as PlannedOp[];
      const outcomes: RunOutcome[] = ops.map((op) => ({
        op_id: op.id,
        ok: true,
        destination: op.destination,
        error: null,
      }));
      if (!args?.dryRun) {
        stubJournal = ops.map((op) => ({
          op_id: op.id,
          rule_id: op.rule_id,
          rule_name: op.rule_name,
          kind: op.kind,
          source: op.source,
          destination: op.destination,
          size: op.size,
          state: "completed",
          started_at: new Date().toISOString(),
          finished_at: new Date().toISOString(),
          error: null,
          source_hash_hex: null,
          undo_status: "available",
        }));
      }
      return outcomes as unknown as T;
    }
    case "journal":
      return stubJournal as unknown as T;
    case "undo": {
      const opId = args?.opId as string;
      const idx = stubJournal.findIndex((e) => e.op_id === opId);
      if (idx >= 0) stubJournal[idx] = {
        ...stubJournal[idx]!,
        state: "undone",
        undo_status: "done",
      };
      return undefined as unknown as T;
    }
    default:
      throw new Error(`stub invoke: unknown command ${cmd}`);
  }
}

export const isStubbed = !tauri;
