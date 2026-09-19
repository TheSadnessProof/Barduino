# Playbook: verifying a change you cannot see (local copy)

Refer to `.agents/skills/verifying-a-ui-change/SKILL.md`.
Core methodology:
- Never drive global mouse/keyboard, never take screenshots, never launch and observe app.
- Move logic out of rendering into testable pure functions.
- Check invisible egui bugs: unsalted widget IDs, mutating session state directly instead of returning panel actions, missing `request_repaint()`, unbounded text in fixed rows.
- Report what user should see accurately and plainly.
