@AGENTS.md

---

## Claude Code specifics

Everything above is imported from `AGENTS.md`, which is the canonical instruction
set for every agent working in this repo. Read it. The notes here are only the
parts that are specific to Claude Code.

### Hooks

`.claude/settings.json` runs `cargo check` and `cargo clippy` after every edit
to a `.rs` file. Together they take under two seconds, so a broken build or a
new clippy warning surfaces immediately rather than three edits later.

There is deliberately **no `cargo fmt` hook** — see rule 3.3 in `AGENTS.md`.
Do not add one.

### Skills

The playbooks live in `.agents/skills/` so that every agent can read them. The
stubs in `.claude/skills/` exist only to make them discoverable here, as
`/adding-a-provider`, `/changing-a-stream-parser` and `/verifying-a-ui-change`.

If you are improving a playbook, edit the file in `.agents/skills/` — the stub
holds no content and must stay that way.

### Plan mode

Worth using before a change that crosses the provider boundary — anything that
touches `agent.rs` plus one of `claude.rs` / `codex.rs` / `antigravity.rs`, or
anything that changes `SavedState`. Those are the two places where a
half-finished change leaves the app broken for the user's existing sessions.

### Running the app

Don't. See rule 3.1: this is a GUI, a human is using this machine, and there is
no screenshot path. The `run` skill does not apply to this repo.
