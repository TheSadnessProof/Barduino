# Playbook: verifying a change you cannot see (local copy)

Viper is a desktop GUI. There is no screenshot path in this codebase, and
a human is using this computer while you work. Taking over their mouse,
keyboard or screen is not a testing strategy; it is an interruption, and it can
click something real.

So: you will finish most UI changes without ever having seen them. This playbook
is about doing that honestly and still being useful.

## Never
- Drive the global mouse or keyboard.
- Take a full-screen capture.
- Launch the app hoping to observe it, or leave it running after you are done.
- Write "verified", "confirmed working" or "looks right" about anything visual.

## Move the logic out of the rendering
Plain functions that take data and return data, tested without egui.

## Invisible bugs to check by reading
- Unsalted widget ids.
- State that outlives the frame mutated from a panel.
- Background work with no repaint (`ctx.request_repaint()`).
- Unbounded text in a fixed row.

## What to do before you hand it back
1. cargo check
2. cargo test
3. cargo clippy --all-targets
4. Re-read diff for invisible bugs
5. Write user short concrete description of what changed.
