# verifying-a-ui-change (Local Copy)
Source: c:\Users\ditob\Documents\viper\.agents\skills\verifying-a-ui-change\SKILL.md

Core methodology:
- Never use screenshots or drive keyboard/mouse.
- Move logic out of rendering into pure functions that take data and return data.
- Test those functions directly.
- Check for invisible egui bugs: unsalted widget IDs, mutating out-of-frame state in panel, background work without repaint, unbounded text.
- Be honest in reporting what was verified and what requires visual confirmation.
