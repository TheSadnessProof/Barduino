---
name: adding-a-provider
description: Add support for a new agent CLI (a new Provider) to Viper. Use when adding, wiring up or removing a provider such as Claude Code, Codex or Antigravity, or when touching Provider::ALL. Covers the 68 dispatch sites across 9 files and the traps the compiler does not catch.
---

# Playbook: adding a new agent CLI

`Provider` is matched at **68 sites across 9 files**. Most of them the compiler
will find for you; a few it will not, and those are where this goes wrong.

Read `AGENTS.md` first. Work in the order below — it is arranged so that
`cargo check` guides you from step 3 onward.

---

## 1. Learn the CLI before writing any Rust

You cannot write the parser from guesswork. Run the real CLI by hand, headless,
and capture what it prints:

```bash
<cli> --help
<cli> <headless flags> > sample.jsonl 2>&1   # ask it something trivial
```

You need to know, concretely:

- The flags for **headless / non-interactive** mode with **streaming JSON** out.
- How the **prompt** is passed. Prefer stdin if it is supported — see the note
  in `agent.rs::start_turn` about quoting and length limits.
- How a **session is resumed** (the flag, and where the id appears in the output).
- How **permissions** are expressed, and what happens when the CLI wants to ask
  the user something it cannot ask in headless mode. Viper's contract is that
  anything needing approval is *denied* rather than left hanging.
- Whether it reports **token usage** and **plan/rate limits**, and where.
- How it reports **models** and **effort levels**, if at all.

Save a real sample as `testdata/<name>_<what>.jsonl`. You will need it in step 4.

## 2. Add the enum variant

In `src/agent.rs`:

```rust
pub enum Provider {
    #[default]
    Claude,
    Codex,
    #[serde(alias = "Gemini")]
    Antigravity,
    NewThing,
}

impl Provider {
    pub const ALL: [Provider; 4] = [Self::Claude, Self::Codex, Self::Antigravity, Self::NewThing];
```

Bump the array length. `ALL`'s order is the order the user sees in pickers.

Now run `cargo check` and let it list the non-exhaustive matches. That list is
your worklist for step 3.

## 3. Fill in the arms the compiler asks for

**`src/agent.rs`** — `label()` (full name as shown in settings), `short_name()`
(the word used in "… is working"), `command()` (the bare executable name),
`install_hint()` (a sentence telling the user where to get it), `find()`
(delegate to your module), and the `parse_line` dispatch in `start_turn()`.

**`src/models.rs`** — `default_efforts()` and `discover()`. If the CLI can list
its own models, write a `<name>_models(exe)` helper the way `codex_models` and
`agy_models` do. If it cannot, hard-code the aliases with a comment saying why,
as `Provider::Claude` does.

**`src/plan.rs`** — `why_missing()`, `check_note()`, `check()`.

## 4. Write the module

Create `src/<name>.rs` and declare it in `main.rs` (the `mod` list is
alphabetical). Mirror the shape of `claude.rs` — it is the clearest of the three:

```rust
//! Runs the <Name> CLI in headless mode and turns its streaming output
//! into events the UI can show.

pub fn find_executable() -> Option<PathBuf>   // PATH first, then usual install dirs
pub fn args(turn: &Turn) -> Vec<String>       // headless flags, model, effort, resume
pub fn parse_line(line: &str) -> Vec<AgentEvent>
```

`parse_line` is pure and total: never panics, returns an empty `Vec` for
anything unrecognised. Index into `serde_json::Value` rather than deriving a
struct — see §4 of `AGENTS.md` for why this is not negotiable.

`find_executable` should check PATH via `agent::find_on_path`, then fall back to
the usual install locations, because an app launched from the Start menu does
not always see a terminal's PATH.

Tests, in the same file: one per `AgentEvent` variant you emit, one reading your
recorded fixture with `include_str!`, and one proving junk input is survivable
(`parse_line("not json")`, `parse_line("")`, an unknown message type).

## 5. The traps the compiler will not catch

Go through these by hand. Every one of them compiles fine while being wrong.

**`plan::is_free`** is written as an inequality, not a match:

```rust
pub fn is_free(provider: Provider) -> bool {
    provider != Provider::Claude
}
```

A new provider is therefore assumed **free** by default. If checking its limits
costs the user anything, you must change this function — nothing will remind you.

**`settings.rs` stores providers the user switched *off***
(`disabled_providers`), deliberately, so that a provider added later starts
switched on. That is the behaviour you want; just be aware your new provider is
enabled for every existing user on upgrade.

**Permission modes.** `PermissionMode` has four variants and your `args()` must
map all of them. Decide explicitly what `ReadOnly`, `AcceptEdits`, `Full` and
`Plan` mean for this CLI, and if it has no real equivalent of one, pick the
*safer* behaviour and say so in a comment. Getting this wrong is how an agent
ends up editing files in a mode the user believed was read-only.

**Stopping the process.** Nothing extra is needed on Windows — `RunningTurn`
puts every child in a job object — but if your CLI spawns a detached helper that
escapes the job, that is worth a test like
`stopping_a_turn_stops_programs_the_agent_started` in `agent.rs`.

**Usage accounting.** If the CLI reports tokens, fill `Usage` properly, including
cache reads and writes separately where they exist. If it does not report them,
leave `usage: None` rather than inventing zeros — the usage view distinguishes
"nothing used" from "not reported".

## 6. Before you call it done

- [ ] `cargo check`, `cargo test`, `cargo clippy --all-targets` — clippy at zero.
- [ ] A `testdata/` fixture from the **real** CLI, with a comment naming the
      version it came from.
- [ ] Junk input returns empty.
- [ ] `is_free` reviewed deliberately.
- [ ] All four permission modes mapped, with a comment on any compromise.
- [ ] An `#[ignore]`d integration test against the real CLI, following the
      pattern in `agent.rs` and documenting its own command in a doc comment.
      **Run it once yourself only if the user asks** — it spends their money.
- [ ] Tell the user what you could not verify. You cannot see the UI; the new
      provider's appearance in the picker and settings is for them to confirm.
