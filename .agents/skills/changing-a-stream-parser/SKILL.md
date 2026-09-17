---
name: changing-a-stream-parser
description: Change how an agent CLI's streaming output is parsed into AgentEvents. Use when editing claude.rs, codex.rs or antigravity.rs, when a CLI changes its JSON format, or when adding a testdata fixture. Covers the tolerant-parsing contract and what every parser must keep proving.
---

# Playbook: changing how a CLI's output is parsed

The three parser modules — `claude.rs`, `codex.rs`, `antigravity.rs` — are the
only place where someone else's output format reaches this codebase. They are
the most likely thing to break without anyone noticing, because a CLI can change
its JSON in a patch release and nothing here fails loudly.

## The contract

```rust
pub fn parse_line(line: &str) -> Vec<AgentEvent>
```

Pure. Total. Never panics. Returns an empty `Vec` for anything it does not
recognise — including blank lines, log noise, progress chatter, and output from
subagents.

The consequence to keep in mind: **degrading gracefully is the whole point.** If
a field moves, the user should see slightly less detail, not a dead app. This is
why the parsers index into `serde_json::Value` instead of deserializing a
struct, and why `unwrap_or_default()` is everywhere.

```rust
// ✅ Null-tolerant: a missing or renamed field yields "" and the app carries on.
match msg["type"].as_str().unwrap_or_default() {
    "assistant" => /* … */,
    _ => Vec::new(),
}

// ❌ One renamed field and every line fails to parse.
#[derive(Deserialize)]
struct Line { r#type: String, session_id: String }
```

## Do not guess the format

Get real output. Run the CLI headless yourself, redirect it to a file, and look
at it. Do not write a test from what you think the JSON looks like — that is how
a parser ends up passing its tests and failing in the app.

When the change is more than a single field, record the output as a fixture:

```
testdata/<provider>_<scenario>.jsonl
```

The existing set shows the naming: `claude_reply_hi.jsonl`,
`codex_edit_file.jsonl`, `codex_failed_turn.jsonl`, `agy_denied.jsonl`,
`agy_read_file.jsonl` — one scenario per file, named for what happens in it.

Note the version in the test's doc comment, because that is the only record of
what the fixture represents:

```rust
/// Real output from Claude Code 2.1.271 answering "hi".
#[test]
fn reads_usage_from_a_recorded_reply() {
    let events: Vec<AgentEvent> =
        include_str!("../testdata/claude_reply_hi.jsonl").lines().flat_map(parse_line).collect();
    let Some(AgentEvent::Finished { usage: Some(usage), error: None, .. }) = events.last() else {
        panic!("expected the turn to finish with usage: {events:?}");
    };
    /* … assert exact figures … */
}
```

Short inline JSON literals are fine when you are checking one field. A whole
turn belongs in `testdata/`.

## What every parser must keep proving

Whatever you change, these must still hold. If a test for them does not exist in
the module you touched, add it.

- `parse_line("")` is empty.
- `parse_line("not json")` is empty.
- An unknown `type` is empty.
- Output belonging to a **subagent** is skipped. `claude.rs` does this by
  checking `parent_tool_use_id`; only the main conversation is shown.
- A turn that fails still produces `Finished` with an `error` the user can read
  — not a silent nothing. `codex_failed_turn.jsonl` exists for this.
- Tools the CLI **refused** land in `denied_tools`, sorted and deduplicated.
  `agy_denied.jsonl` exists for this.

## Events, and what the UI expects of them

| Event | Notes |
| --- | --- |
| `Started` | Session id and model. Emitted once, on init. |
| `TextDelta` | Streaming fragment. |
| `Text` | A finished block, which **replaces** streamed text before it. Do not emit both for the same content unless the CLI genuinely re-sends it. |
| `ToolUse` | `name`, a one-line `detail` (use the shared `agent::tool_detail`), and `edit: Option<FileEdit>` when the CLI says enough to show a diff without reading the file. |
| `ToolResult` | Text plus `is_error`. |
| `Plan` | Plan/limit figures. These belong to the provider, not the session. |
| `Finished` | Session id, optional error, denied tools, optional usage. |
| `Exited` | The process ended; `error` set if it did not exit cleanly. |

For `ToolUse { edit }`: only fill it when both sides of the change are available
from the tool call itself. A write has an empty `old` side, so it reads as an
entirely new file; a read produces no edit at all. `claude.rs::file_edit` is the
reference implementation.

## Done when

- [ ] `cargo check`, `cargo test`, `cargo clippy --all-targets` at zero warnings.
- [ ] A fixture from real output, with the version named in a doc comment.
- [ ] The junk-input and subagent-skipping tests still pass.
- [ ] You did not introduce a `Deserialize` struct for CLI output.
- [ ] You did not run the `#[ignore]`d tests to "check against the real CLI"
      unless the user asked — they spend real account credit.
