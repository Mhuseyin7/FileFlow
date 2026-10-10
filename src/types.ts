// Types mirror the serde shapes emitted by the Rust core. Keep in sync with
// crates/fileflow-core/src/model.rs — a shared codegen is on the roadmap.

export type Category =
  | "documents"
  | "images"
  | "video"
  | "audio"
  | "archives"
  | "installers"
  | "code"
  | "data"
  | "other";

export type NumOp = "lt" | "le" | "eq" | "ge" | "gt";

export type Condition =
  | { type: "source_folder"; path: string; recursive: boolean }
  | { type: "filename_contains"; needle: string }
  | { type: "filename_regex"; pattern: string }
  | { type: "extension"; any_of: string[] }
  | { type: "category"; any_of: Category[] }
  | { type: "mime_type"; any_of: string[] }
  | { type: "size_bytes"; op: NumOp; value: number }
  | { type: "age_days"; op: NumOp; value: number }
  | { type: "hidden"; equals: boolean }
  | { type: "modified_between"; from: string | null; to: string | null }
  | { type: "photo_capture_age_days"; op: NumOp; value: number }
  | { type: "pdf_page_count"; op: NumOp; value: number };

export type ConditionNode =
  | { kind: "all"; children: ConditionNode[] }
  | { kind: "any"; children: ConditionNode[] }
  | { kind: "leaf"; condition: Condition };

export type Action =
  | { kind: "move"; dest_dir: string; rename: string | null }
  | { kind: "copy"; dest_dir: string; rename: string | null }
  | { kind: "rename"; pattern: string }
  | { kind: "create_folder"; path: string }
  | { kind: "tag"; tags: string[] }
  | { kind: "review" }
  | { kind: "trash" };

export type CollisionPolicy = "ask" | "skip" | "rename" | "replace";
export type RuleContinuation = "stop_after_action" | "continue";
export type Trigger = "real_time" | "manual" | "daily" | "weekly";

export interface Rule {
  id: string;
  name: string;
  description: string;
  priority: number;
  enabled: boolean;
  profile: string | null;
  condition: ConditionNode;
  action: Action;
  on_conflict: CollisionPolicy;
  then: RuleContinuation;
  sources: string[];
  trigger: Trigger;
}

export type OpKind =
  | "move"
  | "copy"
  | "rename"
  | "create_folder"
  | "tag"
  | "review"
  | "trash";

export interface PlannedOp {
  id: string;
  rule_id: string;
  rule_name: string;
  kind: OpKind;
  source: string | null;
  destination: string | null;
  size: number;
  collision: CollisionPolicy;
  reasons: string[];
  warnings: string[];
  tags: string[];
}

export interface PreviewSummary {
  count: number;
  total_bytes: number;
  warnings: number;
}

export interface PlanIssue {
  path: string;
  rule_name: string;
  message: string;
}

export interface PreviewResult {
  ops: PlannedOp[];
  summary: PreviewSummary;
  errors: { op_id: string; message: string }[];
  issues: PlanIssue[];
}

export type RunStatus = "done" | "skipped" | "needs_review" | "failed";

export interface RunOutcome {
  op_id: string;
  ok: boolean;
  status: RunStatus;
  destination: string | null;
  error: string | null;
}

export interface ReviewItem {
  id: string;
  path: string;
  rule_id: string;
  rule_name: string;
  kind: string;
  reason: string;
  added_at: number;
}

export interface Recovery {
  op_id: string;
  path: string | null;
  outcome: "not_started" | "completed" | "needs_attention";
  note: string;
}

export type OpState =
  | "executing"
  | "completed"
  | "failed"
  | "skipped"
  | "undoing"
  | "undone";
export type UndoStatus = "not_applicable" | "available" | "unsafe" | "done";

export interface JournalEntry {
  op_id: string;
  rule_id: string;
  rule_name: string;
  kind: OpKind;
  source: string | null;
  destination: string | null;
  size: number;
  state: OpState;
  started_at: string;
  finished_at: string | null;
  error: string | null;
  source_hash_hex: string | null;
  undo_status: UndoStatus;
  dest_modified_ns: number | null;
  replaced: string | null;
}
