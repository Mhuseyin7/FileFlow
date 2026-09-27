# Privacy

FileFlow is designed as a **local-first** utility. Nothing leaves your
computer unless you explicitly export or share it.

## What FileFlow does not do

- **No account.** There is no sign-up, sign-in, or identity of any kind.
- **No cloud.** FileFlow has no server component. Rules, folders, and the
  action journal are stored in a local SQLite database.
- **No filename telemetry.** Names, paths, and file metadata never leave your
  machine.
- **No document uploads.** FileFlow never opens the *contents* of your files
  for classification. Rules match on metadata: name, extension, MIME hint,
  size, dates, category, folder membership.
- **No external AI API calls by default.** Classification is deterministic and
  runs entirely locally.

## What FileFlow stores locally

- **Rules and folders**: what you configured, when it was last updated.
- **Action journal**: for every operation, we store the operation id, rule,
  source path, destination path, size, timestamps, state, and optionally a
  SHA-256 of the source file (only for cross-filesystem moves or when you
  enable verify-on-copy). We store the **hash**, never the file contents.
- **Review queue**: paths of files waiting for your approval.
- **Settings**: app preferences.

That database lives in the platform's app-data directory (for example
`%LOCALAPPDATA%\FileFlow\fileflow.sqlite` on Windows). Deleting the folder
resets FileFlow.

## Diagnostics

Logs are written locally and default to structured, filename-free records
(operation id, rule id, error class). Debug mode can be enabled per-session
via the Settings page; when it is on, filenames may appear in the logs. Debug
mode is off by default and never persists across restarts.

## Import / export

Rule export produces JSON. By default the export **omits absolute personal
paths** and produces a portable template. A "full backup" export includes
those paths and is offered only when you opt in explicitly.

## Optional features that could change the picture

Everything in this section is off by default and requires you to enable it:

- **Native trash integration** would delegate delete to your OS trash rather
  than FileFlow's local `.fileflow-trash/`. Your OS trash may have its own
  telemetry.
- **External AI classification** is not shipped in v1. If it is ever added,
  it will be a clearly labeled opt-in feature that names the provider and the
  data being sent.
