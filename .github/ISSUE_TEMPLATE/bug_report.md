---
name: Bug report
about: Something broke, or FileFlow did something it shouldn't have.
title: "[bug] "
labels: bug
---

**What happened?**
<!-- Describe the misbehavior. Attach the Activity log entry if the issue
     involves a file operation — do NOT paste file *contents*. -->

**What did you expect?**

**Steps to reproduce**
1.
2.
3.

**Rule that was active (if applicable)**
<!-- Export via Rules → … → Export, redact any personal paths. -->
```json
```

**Environment**
- FileFlow version:
- OS and version:
- Install source (installer / cargo / git):

**Safety check**
- [ ] No file was permanently deleted by FileFlow (if it was, this is a **critical** bug — mark the title accordingly).
- [ ] Undo was offered where it should have been (or correctly refused).
