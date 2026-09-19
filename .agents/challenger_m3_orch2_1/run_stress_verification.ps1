# Empirical stress verification runner for Milestone 3
$ErrorActionPreference = "Stop"

Write-Host "=== 1. Checking cargo check ==="
cargo check
if ($LASTEXITCODE -ne 0) { throw "cargo check failed" }

Write-Host "`n=== 2. Running Milestone 3 Lifecycle & State Tests ==="
$m3_tests = @(
    "multi_session_switching",
    "deleting_active_session",
    "folder_change_clears",
    "failed_terminal_spawn",
    "saved_state_ron",
    "comprehensive_legacy",
    "restart_restores",
    "middle_provider_terminal"
)

foreach ($test in $m3_tests) {
    Write-Host "Running $test..."
    cargo test $test
    if ($LASTEXITCODE -ne 0) { throw "Test $test failed" }
}

Write-Host "`n=== 3. Running Provider Interactive Command Builder Tests ==="
cargo test build_interactive_command
if ($LASTEXITCODE -ne 0) { throw "build_interactive_command tests failed" }

Write-Host "`n=== 4. Running Focus Handover & State Resolution Tests ==="
cargo test switching_sessions_primes_keyboard_focus_flag
if ($LASTEXITCODE -ne 0) { throw "switching_sessions_primes_keyboard_focus_flag failed" }

cargo test ready_session_resolves_to_ready_terminal_state
if ($LASTEXITCODE -ne 0) { throw "ready_session_resolves_to_ready_terminal_state failed" }

Write-Host "`n=== 5. Running Process Teardown & Terminal Job Object Stress Tests ==="
cargo test stress_terminal_cleanly_kills_child_and_grandchild_process_tree -- --nocapture
if ($LASTEXITCODE -ne 0) { throw "stress_terminal_cleanly_kills_child_and_grandchild_process_tree failed" }

cargo test stress_terminal_cleanly_kills_batch_script_process_tree -- --nocapture
if ($LASTEXITCODE -ne 0) { throw "stress_terminal_cleanly_kills_batch_script_process_tree failed" }

cargo test stress_terminal_rapid_spawn_and_drop -- --nocapture
if ($LASTEXITCODE -ne 0) { throw "stress_terminal_rapid_spawn_and_drop failed" }

cargo test -- --ignored closing_a_terminal --nocapture
if ($LASTEXITCODE -ne 0) { throw "closing_a_terminal_stops_programs_started_in_it failed" }

Write-Host "`n=== 6. Running Strict Clippy ==="
cargo clippy --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) { throw "cargo clippy failed" }

Write-Host "`n=== All empirical stress tests completed successfully! ==="
