---
name: verifying-a-ui-change
description: Verify a change to Barduino's egui interface without being able to see it. Use for any change a user would see on screen, and before claiming a UI change works. Covers moving logic out of rendering so it can be tested, the invisible egui bugs to read for, and how to report honestly.
---

# Playbook: verifying a change you cannot see

Barduino is a desktop GUI. There is no screenshot path in this codebase, and
**a human is using this computer while you work.** Taking over their mouse,
keyboard or screen is not a testing strategy; it is an interruption, and it can
click something real.

So: you will finish most UI changes without ever having seen them. This playbook
is about doing that honestly and still being useful.

## Never

- Drive the global mouse or keyboard.
- Take a full-screen capture.
- Launch the app hoping to observe it, or leave it running after you are done.
- Write "verified", "confirmed working" or "looks right" about anything visual.

## Instead: move the logic out of the rendering

This is the actual technique, and the codebase is already built for it. Anything
worth verifying should be a plain function that takes data and returns data, with
the `egui` call reduced to drawing the result. Those functions get real tests.

`sidebar.rs` is the model. The grouping rule — one project's sessions stay
together, newest workspace first, newest session first inside it — is a free
function with no `Ui` anywhere near it:

```rust
pub fn group_by_workspace(sessions: &[Session]) -> Vec<Workspace<'_>>
```

and it is tested exactly as you would test any other function:

```rust
#[test]
fn sessions_are_grouped_by_their_folder() {
    let sessions = vec![session(1, "C:\\work\\alpha"), session(2, "C:\\work\\beta"), session(3, "C:\\work\\alpha")];
    let workspaces = group_by_workspace(&sessions);
    let names: Vec<&str> = workspaces.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, ["alpha", "beta"]);
    let alpha_ids: Vec<u64> = workspaces[0].sessions.iter().map(|s| s.id).collect();
    assert_eq!(alpha_ids, [3, 1], "newest first inside a workspace");
}
```

The same split shows up in `line_diff.rs`, `git_diff.rs`, `plan.rs` and
`usage.rs` — diffing, limit parsing and token accounting are all testable
without a window.

**So when you are asked for a UI change, ask what part of it is a rule.** Which
rows appear, in what order, with what label, enabled or not, truncated or not —
those are rules, and rules can be tested. Padding and colour cannot. Push as
much of your change as possible into the first category before you touch a
`Ui`.

If the change is genuinely only visual (spacing, a colour, an icon), say so and
say you could not check it.

## Invisible bugs to check by reading

These compile, pass tests, and look fine in a diff. They are also the bugs you
are most likely to introduce here, so read for them deliberately.

**Unsalted widget ids.** Every widget inside a per-session or per-row view salts
its id with that id. The codebase does this consistently:

```rust
let composer_id = egui::Id::new(("composer", session.id));
egui::ComboBox::from_id_salt(("model", session.id))
egui::ScrollArea::both().id_salt(("diff", &file.path))
ui.scope_builder(egui::UiBuilder::new().id_salt(("session_row", session.id)) /* … */)
```

Omit the salt and two sessions share one widget's state — text typed in one
session's composer shows up in another's, a scroll position jumps, a combo box
opens on the wrong row. Nothing fails; the app is just quietly wrong. If you add
a widget that can appear more than once, salt it.

**State that outlives the frame, mutated from a panel.** Panels return an action
enum; only `app.rs` acts on it. Mutating session state from inside a panel's
`ui()` works today and breaks the moment that panel is drawn twice or drawn for
a different session. See §4 of `AGENTS.md`.

**Background work with no repaint.** egui only redraws when something asks it
to. If your change makes a thread produce results — a CLI event, a model list, a
git scan — the receiving side must call `ctx.request_repaint()`, as `app.rs` does
when agent events arrive. Forget it and the UI appears frozen until the user
happens to move the mouse.

**Unbounded text in a fixed row.** Labels in rows use `.truncate()`, and rows set
their own width first (`ui.set_width(...)`). A long session title or file path
without truncation pushes the rest of the row off the panel.

## What to do before you hand it back

1. `cargo check` — 0.7s.
2. `cargo test` — the whole suite, ~2s.
3. `cargo clippy --all-targets` — still zero warnings.
4. Re-read your diff looking for the four bugs above.
5. Write the user a short, concrete description of what should now be different,
   in terms they can check in one glance. Name the panel, the control, and what
   changed about it.

Something like:

> The session rows in the left panel now show the model name under the title,
> next to the provider, separated by `·`. It truncates with the title if the
> panel is narrow. I could not see this — `cargo check`, 87 tests and clippy all
> pass, and the grouping test still covers the row order. Could you confirm it
> reads the way you wanted at a narrow sidebar width?

That is a genuinely useful report. "Verified the model name displays correctly"
is not, and if you cannot see the window it is also untrue.
