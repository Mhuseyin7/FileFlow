# Contributing to FileFlow

Thanks for your interest. FileFlow is a safety-critical utility — it moves
users' files — so we err on the side of tests over cleverness.

## Local setup

```bash
# Rust
rustup default stable          # 1.77+
cargo test -p fileflow-core    # should print 45 passing tests

# Frontend
npm install
npm run typecheck              # tsc --noEmit
npm run build                  # vite build

# Full desktop app (Tauri v2 prerequisites required)
npm run tauri dev
```

Tauri's system prerequisites (WebView2 on Windows, GTK/WebKit on Linux) only
matter for the desktop shell. The rule engine, planner, safety validator,
and journal are pure Rust and can be developed and tested without them.

## Guiding principles

1. **Never move a file we can't undo.** Any code path that changes the
   filesystem must record its outcome in the journal or be a dry run.
2. **Never overwrite without opting in.** `CollisionPolicy::Replace` is the
   only way to silently overwrite. All other policies must resolve conflicts
   before writing.
3. **The rule engine is deterministic.** Given the same `FileMeta` and
   `now`, `evaluate` returns the same reasons vector every time. If you're
   adding a condition, please add a determinism test.
4. **Sandbox everything.** New action kinds must be validatable against
   `Sandbox`. If it can write outside the allow-list, it doesn't ship.
5. **No AI, no network, by default.** Any feature that would send data
   off-device requires (a) an explicit opt-in UI, (b) a docs update to
   `PRIVACY.md`, and (c) reviewer sign-off.

## Style

- Rust: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`. Comments
  explain *why*, not *what*.
- TypeScript: `strict: true`, `noUnusedLocals`, `noUnusedParameters`,
  `exactOptionalPropertyTypes`. Prefer type imports.
- Tests live next to the code they exercise (Rust: `tests/`; TS: colocated
  `*.test.ts` — none yet, help wanted).
- Commit messages: imperative mood, one sentence + optional body.

## What we especially need help with

- A real `notify` v6 watcher wired into `StabilityWatcher`.
- Scheduled trigger runner for `Trigger::Daily` / `Weekly`.
- Native OS trash integration.
- Windows / macOS / Linux installer pipelines.
- Turkish localization pass on the UI copy (`README_TR.md` is a starting
  point).
- A property-test harness for `rule::evaluate` — the engine is meant to be
  amenable to random-condition-tree testing.

## Reviewing a change that touches the executor

Please attach:

- Tempfile-based tests exercising both the happy path and at least one
  failure mode.
- A note on whether the change alters what gets journaled — the schema in
  `db.rs` and the JS-side `JournalEntry` type must stay in sync.
- A note on what `undo` should do afterwards.
