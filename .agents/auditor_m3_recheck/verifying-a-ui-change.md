# Playbook: verifying a change you cannot see (local copy)
Source: C:\Users\ditob\Documents\viper\.agents\skills\verifying-a-ui-change\SKILL.md

Core methodology:
- Never drive global mouse/keyboard, take full-screen captures, or launch the app expecting to visually inspect it.
- Move logic out of rendering so it is a pure function that can be verified with automated unit tests.
- Guard against invisible egui bugs: unsalted widget IDs, state mutation from inside panel ui(), missing request_repaint() on background threads, unbounded text without truncation.
- Report honestly: describe in plain English what the user should see rather than claiming visual verification.
