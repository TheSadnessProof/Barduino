# Playbook: verifying a change you cannot see (local copy)
Summary: Verify a change to Viper's egui interface without being able to see it.
- Move logic out of rendering
- Check for invisible egui bugs: unsalted widget ids, state mutated from panel, background work with no repaint, unbounded text
- cargo check, cargo test, cargo clippy --all-targets -- -D warnings
- Describe to user what they should see.
