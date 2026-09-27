# Architecture

FileFlow's high-level pipeline is deliberately linear so every stage can be
tested in isolation:

```
File Events  ──►  Normalizer  ──►  Rule Engine  ──►  Action Planner
                                                        │
                                                        ▼
                                                Safety Validator
                                                        │
                                     ┌────── Preview ◄──┤
                                     │                  │
                                     ▼                  ▼
                                UI Renders          Executor
                                                        │
                                                        ▼
                                                     Journal
                                                        │
                                                        ▼
                                                       Undo
```

## Crate layout

```
crates/fileflow-core/     # Pure Rust, no Tauri. Testable in isolation.
    src/model.rs          # FileMeta, Rule, Condition, Action, PlannedOp
    src/category.rs       # Deterministic extension → category
    src/rule.rs           # ConditionNode evaluation, reason trace
    src/rename.rs         # Rename templates + filename sanitization
    src/planner.rs        # Rule → PlannedOp, overlap detection, summary
    src/safety.rs         # Sandbox, symlink escape, invalid dest, race checks
    src/fs_ops.rs         # Atomic rename, cross-fs copy+verify, local trash
    src/journal.rs        # SQLite journal with state machine + undo
    src/db.rs             # Schema + migrations
    src/stability.rs      # Partial-download detection + quiet-window watcher
    src/error.rs          # Unified Error / Result

src-tauri/                # Tauri app. `tauri` feature is opt-in.
    src/main.rs           # Desktop entry point
    src/lib.rs            # Library shim so main and tests can share commands
    src/state.rs          # AppState: journal, rules cache, sandbox, data dir
    src/commands.rs       # IPC-callable functions + #[tauri::command] wrappers

src/                      # React + TypeScript UI
    types.ts              # Mirrors the serde model (kept in sync manually for v1)
    api.ts                # Tauri invoke wrapper + browser dev stub
    App.tsx, main.tsx     # Root
    components/           # Nav, Bytes formatter
    pages/                # Dashboard, Rules, Activity, Review, Folders, Templates, Settings
```

The Tauri feature is off by default so that:

- `cargo test -p fileflow-core` and `cargo build -p fileflow-app` both work
  in CI environments without WebView2 / GTK.
- Contributors can iterate on the rule engine in seconds.

## Data model

`FileMeta` is the normalized view of a file. It's built in one place
(`FileMeta::from_path`) from `std::fs::symlink_metadata` and never from raw
watcher events — the same file is described the same way whether it came in
via an on-demand scan, a real-time watch, or a scheduled sweep.

`Rule` composes:

- A `ConditionNode` tree (`ALL` / `ANY` / `Leaf { Condition }`). Nested groups
  are supported by the engine; the v1 UI exposes a flat "ALL of" list.
- One `Action` (`Move`, `Copy`, `Rename`, `CreateFolder`, `Tag`, `Review`,
  `Trash`).
- A `CollisionPolicy` (`Ask` / `Skip` / `Rename` / `Replace`).
- A `RuleContinuation` (`StopAfterAction` / `Continue`) and a `Trigger`
  (`RealTime` / `Manual` / `Daily` / `Weekly`).

`PlannedOp` is what the planner emits and the executor consumes. It carries
the reasons vector (so the UI simulator can render explanations) and a
warnings vector (populated by the safety validator).

## Rule engine

`rule::evaluate(node, file, now) -> EvalOutcome { matched, reasons }` is
side-effect-free and deterministic. `ALL` visits every leaf even if one
already returned false, so the UI can render a complete reason trace. `ANY`
visits every child too, for the same reason. Extension matching is
case-insensitive; regex errors do not panic.

## Action planner

`planner::plan(rules, files, now)` iterates files once, rules in priority
order, and honors `StopAfterAction` vs `Continue`. It also exposes
`detect_overlaps` which flags rule pairs that share a source folder *and*
overlapping extensions — the UI can nudge the user to reorder them.

## Safety validator

`safety::validate(&sandbox, ops)` runs between the planner and the executor.
It:

- Refuses destinations outside the sandbox.
- Refuses ops in forbidden roots.
- Refuses source-equals-destination no-ops (they'd log a lie).
- Refuses overwrites unless the collision policy is `Replace` (`Rename` and
  `Ask` are also allowed because they'll be resolved to something else).
- Refuses invalid destination filenames (via `rename::sanitize_filename`).
- Refuses symlinks whose target escapes the sandbox.

Blocking errors return; non-blocking issues attach as `warnings` on the
`PlannedOp` and surface in the preview.

## Executor (`fs_ops`)

`execute(op, opts)` implements the state machine:

1. Try `fs::rename`. On success we're done.
2. If `rename` returns `EXDEV` / `ERROR_NOT_SAME_DEVICE`, stream-copy through
   an 8 MiB buffer. Optionally hash both sides with SHA-256. `sync_all` the
   destination. Verify the hash. Then remove the source.
3. On any error, propagate — the executor **never** removes the source
   without the destination being verified.

`resolve_collision` implements the four `CollisionPolicy` values. `Rename`
finds the next `foo (1).ext`, `foo (2).ext`, … slot; `Ask` returns a
`Conflict` error so the UI can prompt.

## Journal + undo

Every op is recorded in the SQLite `journal` table with the state machine:

```
Planned → Executing → Completed | Failed
Completed → Undoing → Undone
```

Undo re-reads the entry, verifies that the file currently at the destination
matches the size (and hash, if recorded) that was captured at execute time,
and only then performs the reverse operation. If verification fails, the
entry's `undo_status` transitions to `unsafe` and the file is left alone.

## Stability watcher

Downloads are the common case where a rule might fire on a half-written
file. `StabilityWatcher::observe(path, size, now)` returns `true` only when:

1. The filename doesn't match a known partial-download suffix
   (`.crdownload`, `.part`, `.partial`, `.download`, `.!ut`, `.!qb`,
   `.opdownload`, `~$…`), AND
2. The size has been unchanged for at least a configurable quiet window.

The current MVP hasn't wired a real `notify`-based watcher yet; the planner
runs against on-demand scans. That's the next module to land.

## What is not in this repo (yet)

- A `notify` v6 wrapper that drives `StabilityWatcher` and dispatches events
  into the pipeline.
- A scheduled trigger runner (real-time is exercised through preview + run
  once).
- Native OS trash. Today `Trash` moves into a per-parent `.fileflow-trash/`
  folder so undo is trivial (`fs::rename` back).
- EXIF / PDF metadata condition types.
- Full internationalization (a placeholder `README_TR.md` exists).
- Installer packaging.

All of these fit the pipeline without changing it: the rule engine, planner,
safety validator, executor, and journal stay as they are.
