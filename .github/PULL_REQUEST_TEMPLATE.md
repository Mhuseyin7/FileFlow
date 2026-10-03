## Summary

<!-- What does this change and why? -->

## Changes

<!-- Bulleted. -->
-

## Safety review

- [ ] No new filesystem operation bypasses the safety validator.
- [ ] Every new executor path records a journal entry on success *and* failure.
- [ ] Undo semantics are documented (or n/a) for new op kinds.
- [ ] No new secret, credential, or absolute personal path is committed.

## Testing

- [ ] `cargo test -p fileflow-core` passes locally.
- [ ] `cargo clippy -p fileflow-core --all-targets --all-features -- -D warnings` is clean.
- [ ] `npm run typecheck && npm run build` passes.
- [ ] Any new behavior has a test.

## Screenshots (UI changes)

<!-- Before / after, light + dark where applicable. -->
