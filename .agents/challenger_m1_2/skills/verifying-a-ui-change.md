# Playbook: verifying a change you cannot see (local copy)
Source: C:\Users\ditob\Documents\viper\.agents\skills\verifying-a-ui-change\SKILL.md

Core methodology: Move logic out of rendering so it can be tested as pure data transformations. Check for invisible egui bugs (unsalted widget IDs, mutated panel state, background work without repaint, unbounded text in fixed rows). Never claim visual verification without eyes; describe what the user should see.
