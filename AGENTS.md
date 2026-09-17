# Barduino — working agreement for coding agents

This file is the canonical instruction set for **every** agent working in this
repository: Claude Code, Codex, Antigravity, Gemini CLI, Cursor, or whatever
comes next. Tool-specific files (`CLAUDE.md`, `GEMINI.md`) only point here.

Read this before your first edit. It is short on purpose, and everything in it
is checkable — no "write clean code" filler.

---

## 1. What this is

Barduino is a **desktop GUI for agentic coding**. It wraps the Claude Code,
Codex and Antigravity CLIs behind one interface, so you can drive any of them
from the same window: sessions, chat, diffs, a terminal, an embedded browser,
plan/usage tracking.

- Pure Rust. `eframe`/`egui` for the UI. **Windows first**, macOS second.
- ~7,700 lines in one crate, one flat module per concern in `src/`.
- No async runtime. Threads and `std::sync::mpsc` channels.
- Deliberately dependency-light. See rule 3.

The thing you are editing is a GUI that a human uses while you work. That single
fact drives most of the rules below.

---

## 2. The loop, every time

```
read the neighbours  →  edit  →  cargo check  →  cargo test  →  cargo clippy
```

| Command | Time | What it is for |
| --- | --- | --- |
| `cargo check` | **0.7s** warm | After *every* edit. No excuse to skip it. |
| `cargo test` | **~2s** | Before you claim anything works. |
| `cargo clippy --all-targets` | **~1s** | Currently **zero warnings**. Keep it there. |

Clippy being at zero is a real invariant, not an aspiration. If your change adds
a warning, you fix the warning — you do not add `#[allow(...)]` to silence it
unless you explain why in a comment.

**A commit hook enforces both.** `.githooks/pre-commit` runs clippy with
`-D warnings` and the test suite, and refuses the commit if either fails. It is
in the repo rather than in `.git/hooks` so it applies to whoever is committing,
whichever tool they are driving. Enable it once per clone:

```
git config core.hooksPath .githooks
```

Do not reach for `--no-verify`. If the hook is in your way, the build is broken
and that is the thing to fix.

---

## 3. Hard rules

These are the ones that break the app, cost the user real money, or bury them in
diff noise. Violating any of them means your change gets reverted.

### 3.1 You cannot see this app. Do not try.

There is no screenshot path in this codebase, and **a human is using this
machine while you work**.

- ❌ Never drive the global mouse or keyboard.
- ❌ Never take a full-screen capture.
- ❌ Never launch the app expecting to observe it, and never leave it running.

To verify a UI change: `cargo check`, then `cargo test`, then **describe in
words what the user should now see and ask them to look**. Saying "I verified
the button appears" when you cannot see the button is a lie, and it is worse
than saying nothing. See the `verifying-a-ui-change` skill.

### 3.2 Never run the ignored tests wholesale

The `#[ignore]`d tests reach outside the process, and they are **not all the
same kind**. Every one of them says which it is in its own doc comment — read
that before running anything.

- **Some spend the user's money**, driving a real CLI against a real paid
  account: `runs_the_real_antigravity_cli`, `runs_the_real_codex_cli`,
  `full_access_really_runs_commands`. Run these **only when the user asks**.
- **The rest are free but depend on this computer** — what is installed, signed
  in, or running: `real_models_come_from_the_clis`, `checks_real_plan_limits`,
  `reads_codex_limits_from_this_computer`, `finds_the_shells_on_this_computer`,
  `closing_a_terminal_stops_programs_started_in_it`. These are fine to run when
  they are what you actually need to check.

Either way:

- ❌ `cargo test -- --ignored` — that sweeps up the paid ones too.
- ❌ Removing an `#[ignore]` to "check it passes".
- ✅ The exact command in that test's own doc comment, e.g.
  `cargo test -- --ignored antigravity --nocapture`.

A new `#[ignore]`d test must say in its doc comment which kind it is and how to
run it.

### 3.3 Do not run `cargo fmt`

The house style is wider and more compact than rustfmt produces. `rustfmt.toml`
pins the closest fit, but even then `cargo fmt` rewrites ~456 lines it has no
business touching (it explodes single-line struct variants, joins method chains
the author split on purpose).

Match the style of the code around you by hand. If you genuinely need to format
something, format only the lines you wrote.

### 3.4 Do not add dependencies

Every entry in `Cargo.toml` is there for a stated reason, several with a comment
explaining it. In particular, this codebase intentionally has **no**
`anyhow`, no `thiserror`, no `tokio`.

If you believe a dependency is needed, stop and ask. Do not add one and mention
it afterwards.

### 3.5 Do not break saved state

`SavedState` is persisted as RON by eframe. Users have existing sessions on disk.

- Adding a field: give it a `#[serde(default)]` or a `Default` so old saves load.
- Renaming a variant: keep a `#[serde(alias = "OldName")]`.
- There is already a live example: `Provider::Antigravity` carries
  `#[serde(alias = "Gemini")]` because sessions were saved when Gemini CLI was
  the third option. **Do not remove that alias.** There is a test guarding it.

---

## 4. Architecture

```
main.rs        window setup, global text sizes
app.rs         BarduinoApp — owns all state, routes panel actions
agent.rs       the shared surface: Provider, AgentEvent, PermissionMode,
               Turn, and spawning one turn as a child process
claude.rs  ┐
codex.rs   ├─  one per CLI: find_executable(), args(), parse_line()
antigravity.rs ┘
session.rs     Session, Entry — one conversation and its history
chat.rs        the middle column: transcript + composer
sidebar.rs     session list
tools.rs       the right-hand tool panel
settings.rs    settings pages, CLI detection
plan.rs        plan/rate-limit reading, per provider
usage.rs       token accounting
git_diff.rs    reading the working tree
line_diff.rs   FileEdit and line-level diffing
changes.rs     the changed-files view
terminal.rs    embedded PTY (portable-pty + vt100)
browser.rs     embedded WebView (wry) + picker.js
models.rs      model catalogue
icons.rs       icon buttons and toggles
testdata/*.jsonl   recorded real CLI output, used by parser tests
```

### Two patterns you must follow

**Panels return actions; they never mutate app state.** Every panel's `ui()`
returns an action enum with a `None` variant, and `app.rs` is the only place
that acts on it:

```rust
pub enum SidebarAction {
    None,
    Select(u64),
    NewSession,
    Rename(u64, String),
    Delete(u64),
    /* … */
}

pub fn ui(&mut self, ui: &mut egui::Ui, /* … */) -> SidebarAction
```

A panel may own *ephemeral* state (which row is being renamed, which delete is
awaiting confirmation) — that is what `Sidebar`'s fields are. Anything that
outlives the frame or belongs to a session goes through an action.

**CLI output is parsed tolerantly.** `parse_line(&str) -> Vec<AgentEvent>` is a
pure, total function. It must never panic and must return an empty `Vec` for
anything it does not recognise.

```rust
// ✅ indexing a Value yields Null instead of panicking
let session_id = string(&msg["session_id"]);
match msg["type"].as_str().unwrap_or_default() { /* … */ }

// ❌ never do this — these formats change under us without warning
#[derive(Deserialize)] struct ClaudeLine { r#type: String, session_id: String }
```

These CLIs change their JSON between releases. Tolerant indexing degrades to
"we show a little less"; a strict struct degrades to "the app is broken".

---

## 5. House style

The existing code has a strong, consistent voice. Match it. Read the file you
are editing before you add to it — that is not a formality here, the
conventions below are visible in every file.

### 5.1 Comments explain *why*, never *what*

Every module opens with a `//!` line saying what it is for:

```rust
//! The left-hand panel: the session list, with rename and delete.
//! Runs the Claude Code CLI in headless mode and turns its streaming JSON
//! output into events the UI can show.
```

Inline comments justify a decision the reader would otherwise question:

```rust
// Claude reads the prompt from stdin, which avoids command-line quoting and
// length limits. Dropping stdin afterwards tells the CLI the prompt is complete.

// Lock only briefly so a Stop click is never stuck behind this loop.

// Apps launched from the Start menu don't always see the PATH a terminal has.
```

The contrast that matters:

```rust
// ❌ let mut plain = String::new();   // create a string
// ❌ fn error_summary(...)            // summarises the error

// ✅ /// The useful part of a CLI's error output: the last message lines,
// ✅ /// without stack traces or terminal color codes.
```

Doc comments on public items describe *intent and usage*, and are exact about
return semantics:

```rust
/// The short name used in sentences like "Claude is working…".
pub fn short_name(self) -> &'static str

/// A text box for renaming. Returns `Some(Some(name))` when saved, `Some(None)`
/// when cancelled, and `None` while the user is still typing.
fn rename_row(...) -> Option<Option<String>>
```

Full sentences. Capital letter, full stop. Prose, not telegram.

### 5.2 User-facing text is plain English

This app talks to a human, and the strings are written like it:

```rust
"The agent can read the project but not change it or run commands."
"The agent works out a plan and doesn't change anything."
"Install it from https://claude.com/claude-code"
"These sessions still need a folder"
```

No jargon, no abbreviations, no `Err:` prefixes. Sentences for descriptions,
bare phrases for labels and tooltips.

Use typographic punctuation, not ASCII substitutes — the codebase already does
throughout: `…` not `...`, `·` not `-` as a separator, `“ ”` for quoted names
(`format!("Delete “{}”?", session.title)`).

### 5.3 Errors are messages for the user, not types

There is no error enum. A failure becomes a `String` a human can act on:

```rust
/// Turns a failed CLI run into a message the user can act on.
fn exit_error(provider: Provider, code: Option<i32>, stderr: &str) -> String
```

Best-effort side effects that genuinely cannot be handled are swallowed
explicitly with `let _ =`, never with `.unwrap()`:

```rust
let _ = child.kill();
let _ = stdin.write_all(prompt.as_bytes());
```

Locks use `unwrap_or_else(PoisonError::into_inner)` — a panicked thread must not
take the UI down with it:

```rust
let mut child = self.child.lock().unwrap_or_else(PoisonError::into_inner);
```

`.expect()` is allowed **only** for an invariant established a line or two
above, and the message states the invariant:

```rust
let mut stdin = child.stdin.take().expect("stdin is piped");
```

### 5.4 Tests

Inline `#[cfg(test)] mod tests` at the bottom of the file. No `tests/` directory.

**Name tests as sentences describing the behaviour:**

```rust
✅ fn sessions_are_grouped_by_their_folder()
✅ fn an_edit_carries_both_sides_of_the_change()
✅ fn skips_subagent_output_and_noise()
✅ fn full_access_bypasses_permission_prompts()
❌ fn test_parse()
❌ fn test_sidebar_2()
```

**Destructure with slice patterns and a prose panic:**

```rust
let [AgentEvent::ToolUse { edit: Some(edit), .. }] = &parse_line(line)[..] else {
    panic!("an edit should come through")
};
```

**Assert messages read as prose, and dump the value on failure:**

```rust
assert_eq!(alpha_ids, [3, 1], "newest first inside a workspace");
assert!(args.windows(2).any(|pair| pair == ["--model", "opus"]), "{args:?}");
```

**Comments in tests explain the scenario, not the mechanics:**

```rust
// Beta holds the newest session that isn't alpha's, but alpha's newest is newer.
// A write has nothing on the old side, so it reads as a new file.
// Reading a file changes nothing, so there is no diff to show.
```

**Real CLI output goes in `testdata/`, not in your head.** When you touch a
parser, record actual output and note the version it came from:

```rust
/// Real output from Claude Code 2.1.271 answering "hi".
#[test]
fn reads_usage_from_a_recorded_reply() {
    let events: Vec<AgentEvent> =
        include_str!("../testdata/claude_reply_hi.jsonl").lines().flat_map(parse_line).collect();
    /* … */
}
```

Short hand-written JSON literals inline are fine for a single field. A whole
turn belongs in a fixture. Every parser must have tests proving it survives
junk: `parse_line("not json")`, `parse_line("")`, and an unknown message type
all return empty.

### 5.5 Rust idiom

Edition 2024. The codebase uses modern control flow and expects you to:

```rust
let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else { return Vec::new() };

if let Some((id, name)) = &mut self.renaming
    && *id == session.id
{ /* … */ }

let count = |key: &str| usage[key].as_u64().unwrap_or(0);
```

Small local closures instead of tiny private helpers. Iterator chains over
manual loops where it reads better. `#[cfg(windows)]` blocks for
platform-specific work, with a `#[cfg(not(windows))]` stub that degrades
gracefully rather than failing to compile.

---

## 6. Playbooks (skills)

Multi-file procedures that are easy to get half-right live in `.agents/skills/`,
following the [Agent Skills](https://agentskills.io) standard. If your tool
discovers skills, it will offer these automatically. If it does not, **read the
file yourself before starting that kind of change** — they are plain Markdown:

| If you are… | Read |
| --- | --- |
| Adding support for a new agent CLI | `.agents/skills/adding-a-provider/SKILL.md` |
| Changing how a CLI's output is parsed | `.agents/skills/changing-a-stream-parser/SKILL.md` |
| Changing anything the user sees | `.agents/skills/verifying-a-ui-change/SKILL.md` |

---

## 7. Definition of done

Do not report a change as complete until all of these are true:

- [ ] `cargo check` passes.
- [ ] `cargo test` passes, with nothing newly ignored.
- [ ] `cargo clippy --all-targets` is still at **zero** warnings.
- [ ] New behaviour has a test, named as a sentence.
- [ ] Touched a parser? A `testdata/` fixture covers it, and junk input still
      returns empty.
- [ ] New module? It opens with a `//!` line.
- [ ] Non-obvious decision? A comment says *why*.
- [ ] New user-facing string? Plain English, typographic punctuation.
- [ ] You did **not** run `cargo fmt`, and the diff contains no reformatting of
      lines you did not otherwise change.
- [ ] You did **not** run the ignored tests.
- [ ] UI change? You described what the user should see and asked them to look,
      rather than claiming you verified it visually.

If you could not finish something, say so plainly and say why. A clear "this
part is untested because I cannot see the window" is worth more than a confident
summary that does not hold up.

---

## 8. Where these instructions live

**This file is the only copy.** Everything else points at it. If you are
improving the guidance, edit `AGENTS.md` or a skill — never a shim.

| File | Read by | Contains |
| --- | --- | --- |
| `AGENTS.md` | Codex, Cursor, Antigravity, Copilot (GitHub.com + VS Code) — natively | everything |
| `.agents/skills/*/SKILL.md` | Codex, Cursor, Gemini CLI, Copilot, Antigravity | the playbooks |
| `CLAUDE.md` | Claude Code | `@AGENTS.md` import + Claude-only notes |
| `.claude/skills/*/SKILL.md` | Claude Code | four-line stubs pointing at `.agents/skills/` |
| `.claude/settings.json` | Claude Code | the check/clippy hook |
| `.agents/rules/main.md` | Antigravity | one-line import of this file |
| `.gemini/settings.json` | Gemini CLI | makes it read `AGENTS.md` |

Not currently present: `.github/copilot-instructions.md`, which is the only file
Copilot reads in JetBrains, Xcode and Eclipse, and which has no include
mechanism. If anyone starts using Copilot in those IDEs, that file has to be
generated from this one rather than hand-maintained.

Keep this file under **32 KB**. Codex concatenates `AGENTS.md` files up to a
32 KiB budget and silently drops what overflows. The skills are separate files
and do not count against it unless loaded.
