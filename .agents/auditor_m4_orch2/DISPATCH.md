## 2026-09-19T02:35:05Z

You are auditor_m4_orch2.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\auditor_m4_orch2

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md

OBJECTIVE:
Execute the Final Comprehensive Forensic Integrity Audit for the entire Viper Interactive Provider Terminal project.

AUDIT CHECKS (ALL MANDATORY):
1. Static Analysis & Code Layout:
   - Inspect all touched files: `src/terminal.rs`, `src/agent.rs`, `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`, `src/app.rs`, `src/chat.rs`.
   - Confirm all implementations are authentic, functional, and genuine.
   - Check for cheating: zero hardcoded test outputs, zero dummy facades, zero mock bypasses.
   - Confirm `Cargo.toml` has ZERO added dependencies.
   - Confirm zero mass-reformatting on untouched lines (`AGENTS.md` Rule 3.3).
   - Confirm NO auto-commits were executed (`AGENTS.md` Rule 3.6).
2. Runtime & Process Verification:
   - Confirm PTY processes are genuinely spawned via `portable-pty`.
   - Confirm process tree termination uses Windows Job Objects (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) on Windows and process groups on Unix.
   - Confirm SavedState strictly omits PTY handles and maintains 100% RON backward compatibility (`AGENTS.md` Rule 3.5).
3. Verification Suite:
   - `cargo check`: must compile with 0 errors.
   - `cargo test`: all 284+ unit tests pass, zero newly ignored tests, exactly the 8 historical system tests remain ignored per Rule 3.2.
   - `cargo clippy --all-targets -- -D warnings`: must pass with EXACTLY ZERO warnings.
4. Acceptance Criteria Verification:
   - R1: Interactive terminal replacing chat in central panel while keeping sidebar & tools panel intact.
   - R2: Direct PTY provider execution with interactive argument builder, ConPTY batch wrapping, TrueColor/xterm-256color, dynamic resize, focus lock.
   - R3: Session lifecycle, per-session PTY map, switching, immediate focus, process cleanup on delete/folder change, SavedState backward compatibility.
   - R4: Full retention of left sidebar and right auxiliary tools panel.

CONSTRAINTS:
- Read-only. DO NOT edit source code files.
- Deliver full audit evidence report to `c:\Users\ditob\Documents\viper\.agents\auditor_m4_orch2\handoff.md`.
- Explicitly state final verdict: CLEAN or INTEGRITY VIOLATION.
- Send message to parent reporting verdict and summary.
