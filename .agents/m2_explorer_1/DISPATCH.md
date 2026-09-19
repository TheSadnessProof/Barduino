## 2026-09-19T01:45:00Z
You are m2_explorer_1.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\m2_explorer_1

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md
c:\Users\ditob\Documents\viper\.agents\skills\verifying-a-ui-change\SKILL.md

YOUR OBJECTIVE:
Provide a precise technical design and blueprint for Milestone 2: Replacing `chat_area` with `terminal_area` in `src/app.rs`.

SCOPE & INVESTIGATION:
1. Examine `src/app.rs` lines 530–620 (`chat_area`) and 810–830 (`CentralPanel` rendering).
2. Design `fn terminal_area(&mut self, ui: &mut egui::Ui)`:
   - What happens when a session has no folder (`!session.has_folder()`)? Design the clean folder selection UI ("Choose a project folder to start") with folder change action.
   - What happens when the provider executable is missing (`self.detected.get(session.provider).is_none()`)? Design the missing CLI banner with install guidance and "Open Settings" button.
   - What happens when ready? How the terminal is spawned (or fetched from runtime map) and rendered via `terminal.ui(ui, take_keyboard)`.
3. Ensure the left panel (`self.left_panel(ui)`) and right tools panel (`self.right_panel(ui, frame)`) remain intact and untouched.
4. Provide exact code snippets and line-by-line diffs for the worker.

CONSTRAINTS:
- Read-only. DO NOT edit files outside your working directory.
- Deliver findings in `c:\Users\ditob\Documents\viper\.agents\m2_explorer_1\handoff.md`.
- Send message to parent upon completion.
