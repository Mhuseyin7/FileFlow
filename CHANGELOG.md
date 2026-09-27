# Changelog

All notable changes to this project will be documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Real-time watcher** (`fileflow_core::watcher`) wrapping `notify` v6 with
  a coalescing debouncer and stability gate. Partial-download suffixes and
  paths inside `.fileflow-trash/` are filtered before events escape the
  pipeline.
- **Scheduler** (`fileflow_core::scheduler`) for `Trigger::Daily` /
  `Trigger::Weekly` rules. Pure `is_due` decision function is testable
  without a running thread; the running scheduler emits `ScheduleTick`s via
  a crossbeam channel.
- **Native OS trash** via the `trash` crate (feature-gated behind
  `os-trash`, on by default). `ExecuteOpts::use_os_trash` picks OS trash
  vs. FileFlow's local `.fileflow-trash/`. OS trash undo is not offered
  because the destination path isn't knowable on all platforms; local
  trash retains full-fidelity undo.
- **Optional EXIF / PDF metadata conditions** (feature `metadata`, on by
  default). New `Condition::PhotoCaptureAgeDays` and
  `Condition::PdfPageCount` read metadata locally; the file's *contents*
  are never uploaded.
- **Turkish UI localization**. `src/i18n.ts` — dict + `useT` hook with
  automatic detection from `navigator.language` and persisted user choice
  in localStorage. Settings page ships a language switcher.
- **Installer packaging config**. Per-platform bundle targets in
  `src-tauri/tauri.conf.json` (NSIS + MSI on Windows, DMG on macOS,
  DEB/AppImage/RPM on Linux) plus a `RELEASING.md` runbook.
- New Tauri commands: `start_watcher`, `stop_watcher`, `watcher_running`.
- Tests: scheduler (5), watcher (2 end-to-end through real notify), metadata
  (2 including a real PDF built in-test with lopdf), fs_ops OS-trash path
  (opt-in via `FILEFLOW_SKIP_OS_TRASH`).

### Changed
- `ExecuteOpts` gained a `use_os_trash` field. Existing callers keeping
  `..Default::default()` retain the previous behavior; per-test setups were
  updated.

### v0.1 initial ship

- Initial scaffold: Cargo workspace, Vite + React + TypeScript strict,
  Tailwind, ESLint.
- `fileflow-core` crate: model, category, rule engine, rename templates,
  planner, safety validator, filesystem operations (atomic rename +
  cross-filesystem copy/verify/delete), local trash, journal with state
  machine, undo with identity verification, SQLite schema, stability
  watcher.
- 45+ Rust tests covering rule evaluation, planner, safety, filesystem
  operations (with tempfiles), journal + undo, and stability windows.
- Tauri v2 command layer behind a `tauri` feature flag so the core can be
  tested without WebView2 / GTK.
- React UI: Dashboard, Rules editor with live preview + dry-run, Activity
  with undo, Review queue placeholder, Folders (sandbox management),
  Templates (safe disabled starting points), Settings.
- Docs: README (EN + TR placeholder), PRIVACY, SECURITY, ARCHITECTURE,
  CONTRIBUTING.
