# Security

FileFlow's threat model is a normal desktop user with a normal
non-administrator account. The value we protect: **not corrupting or losing
data**, and **not touching files outside the folders the user granted**.

## Reporting

Please email security reports to `security@fileflow.local` (placeholder
until the project has a maintained mailbox). Give us a reasonable window to
land a fix before disclosure.

## Runtime posture

- **Runs unprivileged.** FileFlow never asks for administrator or root. If an
  action fails because of OS permissions, the error surfaces in the UI; we
  do not attempt to escalate.
- **Sandboxed roots.** Only folders you explicitly grant on the *Folders*
  page can be sources or destinations. The safety validator refuses any op
  whose source or destination canonicalizes outside the allow-list.
- **Forbidden roots.** System directories (Windows / System32, macOS
  /System, Linux /proc, …) can be flagged as forbidden even inside the
  allow-list.
- **No shell.** FileFlow never shell-concatenates paths, never executes a
  user-supplied script, and rules cannot invoke external programs.

## Filesystem correctness

- **Symlink escape.** Before executing a move or copy, the safety validator
  resolves any symlink and refuses if the resolved target sits outside the
  sandbox.
- **Junction loops.** The scanner ignores symlinks entirely rather than
  following them.
- **Same-filesystem moves** use `std::fs::rename` (atomic on POSIX and
  Win32).
- **Cross-filesystem moves** stream through an 8 MiB buffer with a SHA-256
  hasher on both source and destination. The source is only removed after
  the destination hash matches. If it doesn't, the destination is deleted
  and the operation fails.
- **Race safety.** Undo does not blindly restore. It re-hashes and re-sizes
  the file at the destination before moving it back; if either has changed,
  the undo is aborted with `undo_status = unsafe`.
- **Reserved filenames.** Rendered names are sanitized against Windows'
  reserved device names (`CON`, `NUL`, `COM1`, …), trailing dots/spaces, and
  disallowed characters.

## Threats we do *not* claim to defend against

- A malicious user with write access to the same account and DB file could
  corrupt the journal.
- A malicious filesystem driver could lie about file identity.
- If a rule's destination path is granted, FileFlow will use it. FileFlow
  cannot prevent you from configuring a rule that legitimately overwrites
  files in a place you granted — but it will still refuse silent overwrite
  unless the collision policy is `Replace`.

## Test coverage

The `fileflow-core` crate ships 45+ tests covering:

- rule evaluation (case sensitivity, boundary comparisons, empty groups,
  regex error paths, determinism);
- action planning (priority, stop-vs-continue, source restriction, overlap
  detection);
- rename templates (tokens, sanitization, reserved names, empty inputs);
- filesystem operations (atomic rename, dry-run isolation, cross-fs
  copy+verify, collision policies, local trash, next-available naming);
- safety validator (destination outside sandbox, forbidden root, invalid
  filename, source==destination, replace policy);
- journal & undo (record + undo, refusal on size mismatch, refusal on
  reoccupied source);
- file stability (partial-download suffixes, first-observation, quiet
  period, size-change reset).

Run them with:

```bash
cargo test -p fileflow-core
```
