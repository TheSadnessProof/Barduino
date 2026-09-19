## 2026-09-19T02:35:05Z

You are challenger_m4_orch2_2.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\challenger_m4_orch2_2

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md

CRITICAL INSTRUCTION ON WORKSPACE MUTATION:
DO NOT MODIFY ANY PERMANENT SOURCE CODE FILES IN `src/`. DO NOT append test blocks or edit files in `src/`. Run tests via `cargo test` or standalone verification scripts in your working directory.

OBJECTIVE:
Adversarial coverage and edge-case verification for Milestone 4.

SCOPE & STRESS TESTING:
1. Adversarial verification of:
   - Dynamic viewport resizing across extremes (0x0, 1x1, 4K/5K viewports).
   - SavedState serialization and legacy RON backward compatibility across version schemas.
   - Multi-session concurrent switching permutations.
   - Middle provider terminal and right tools panel coexistence under rapid tab switching.
2. Confirm `cargo check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings` pass cleanly.

CONSTRAINTS:
- DO NOT edit permanent codebase files.
- Deliver findings in `c:\Users\ditob\Documents\viper\.agents\challenger_m4_orch2_2\handoff.md`.
- Send message to parent reporting verdict (APPROVE or REQUEST_CHANGES).
