//! Milestone 2 Unit Test Suite Proposal
//!
//! Designed by m2_explorer_3 in accordance with AGENTS.md §5.4 and verifying-a-ui-change SKILL.md.
//!
//! This file contains:
//! 1. Architecture helper types (`calculate_dimensions`, `CentralState`, `active_session`)
//! 2. Unit tests for `src/terminal.rs`
//! 3. Unit tests for `src/app.rs`

// ==============================================================================
// 1. HELPERS TO BE ADDED TO CORE MODULES
// ==============================================================================

/*
In `src/terminal.rs`:

/// Calculates the number of terminal rows and columns that fit into a given pixel size.
pub fn calculate_dimensions(size: Vec2, char_width: f32, row_height: f32) -> (u16, u16) {
    let rows = ((size.y - 2.0 * PADDING) / row_height).floor().max(2.0) as u16;
    let cols = ((size.x - 2.0 * PADDING) / char_width).floor().max(10.0) as u16;
    (rows, cols)
}

And inside `Terminal::ui`:
Replace:
    let rows = ((rect.height() - 2.0 * PADDING) / row_height).floor().max(2.0) as u16;
    let cols = ((rect.width() - 2.0 * PADDING) / char_width).floor().max(10.0) as u16;
With:
    let (rows, cols) = calculate_dimensions(rect.size(), char_width, row_height);
*/

/*
In `src/app.rs`:

/// What the central area displays for the active session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CentralState {
    /// The active session does not have a project folder chosen yet.
    NeedsFolder,
    /// The active session has a folder, but its provider CLI is not installed.
    MissingCli(Provider),
    /// The session has a folder and detected CLI executable.
    TerminalReady,
}

impl ViperApp {
    /// Determines the display state of the central view for the active session.
    pub(crate) fn central_state(&self) -> CentralState {
        let index = self.active_index();
        let session = &self.state.sessions[index];
        if !session.has_folder() {
            CentralState::NeedsFolder
        } else if self.detected.get(session.provider).is_none() {
            CentralState::MissingCli(session.provider)
        } else {
            CentralState::TerminalReady
        }
    }

    fn active_session(&self) -> &Session {
        let index = self.active_index();
        &self.state.sessions[index]
    }
}
*/

// ==============================================================================
// 2. UNIT TESTS FOR `src/terminal.rs` (under #[cfg(test)] mod tests)
// ==============================================================================

#[cfg(test)]
mod terminal_milestone2_tests {
    use super::*;
    use eframe::egui::Vec2;

    #[test]
    fn terminal_dimensions_are_calculated_from_allocated_rect_and_font_metrics() {
        // Standard monospace font metrics: 8px glyph width, 16px row height
        let char_width = 8.0;
        let row_height = 16.0;

        // Allocated rect: 800px wide, 600px high
        // Usable width: 800 - 2 * 6.0 = 788px -> floor(788 / 8) = 98 cols
        // Usable height: 600 - 2 * 6.0 = 588px -> floor(588 / 16) = 36 rows
        let dims = calculate_dimensions(Vec2::new(800.0, 600.0), char_width, row_height);
        assert_eq!(dims, (36, 98), "800x600 allocated rect produces 36 rows and 98 columns");

        // Window resize wider: 1200px wide, 600px high
        // Usable width: 1200 - 12 = 1188px -> floor(1188 / 8) = 148 cols
        let wider_dims = calculate_dimensions(Vec2::new(1200.0, 600.0), char_width, row_height);
        assert_eq!(wider_dims, (36, 148), "widening window from 800 to 1200 increases columns from 98 to 148");

        // Sidebar expanded, narrowing terminal: 500px wide, 600px high
        // Usable width: 500 - 12 = 488px -> floor(488 / 8) = 61 cols
        let narrower_dims = calculate_dimensions(Vec2::new(500.0, 600.0), char_width, row_height);
        assert_eq!(narrower_dims, (36, 61), "narrowing terminal due to sidebar expansion decreases columns to 61");

        // Window height increased: 800px wide, 900px high
        // Usable height: 900 - 12 = 888px -> floor(888 / 16) = 55 rows
        let taller_dims = calculate_dimensions(Vec2::new(800.0, 900.0), char_width, row_height);
        assert_eq!(taller_dims, (55, 98), "increasing height to 900 increases rows to 55");
    }

    #[test]
    fn terminal_resizing_clamps_to_minimum_rows_and_columns_on_tiny_or_zero_rect() {
        let char_width = 8.0;
        let row_height = 16.0;

        // Zero size: must clamp to minimum 2 rows and 10 columns without underflowing
        let zero_dims = calculate_dimensions(Vec2::ZERO, char_width, row_height);
        assert_eq!(
            zero_dims,
            (2, 10),
            "zero allocated size clamps to minimum dimensions (2 rows, 10 cols) without underflowing"
        );

        // Tiny size smaller than padding (5x5 px)
        let tiny_dims = calculate_dimensions(Vec2::new(5.0, 5.0), char_width, row_height);
        assert_eq!(
            tiny_dims,
            (2, 10),
            "allocated size smaller than padding clamps to minimum 2 rows and 10 columns"
        );

        // Exact padding boundary: 12x12 px gives 0 usable space
        let boundary_dims = calculate_dimensions(Vec2::new(12.0, 12.0), char_width, row_height);
        assert_eq!(
            boundary_dims,
            (2, 10),
            "exact padding boundary (12x12) clamps to minimum dimensions"
        );
    }

    #[test]
    fn terminal_start_command_resizes_underlying_pty_and_parser_screen() {
        let temp = std::env::temp_dir();
        let (prog, args) = if cfg!(windows) {
            (PathBuf::from("cmd.exe"), vec!["/c".to_owned(), "exit".to_owned(), "0".to_owned()])
        } else {
            (PathBuf::from("true"), Vec::new())
        };

        let mut terminal = Terminal::start_command(&temp, &prog, &args, egui::Context::default())
            .expect("terminal should start");

        assert_eq!(terminal.size, (24, 80), "initial terminal size is 24x80");
        assert_eq!(terminal.parser().screen().size(), (24, 80), "parser screen initializes to 24x80");

        // Simulate resize to (40 rows, 120 cols)
        let new_size = (40u16, 120u16);
        let _ = terminal.master.resize(pty_size(new_size));
        terminal.parser().screen_mut().set_size(new_size.0, new_size.1);
        terminal.size = new_size;

        assert_eq!(terminal.size, (40, 120), "terminal size field reflects new grid dimensions");
        assert_eq!(terminal.parser().screen().size(), (40, 120), "parser screen updates to 40x120");
    }
}

// ==============================================================================
// 3. UNIT TESTS FOR `src/app.rs` (under #[cfg(test)] mod tests)
// ==============================================================================

#[cfg(test)]
mod app_milestone2_tests {
    use super::*;

    #[test]
    fn central_view_renders_terminal_area_without_chat_composer() {
        let mut state = SavedState::default();
        let session = Session::new(10, PathBuf::from(r"C:\work\project"), Provider::Claude, PermissionMode::ReadOnly);
        state.sessions = vec![session];
        state.active_session = 10;
        state.next_session_id = 11;

        let mut app = ViperApp::test_app(state);
        assert_eq!(app.view, View::Terminal, "central view defaults to terminal view rather than settings");

        // Execute a headless egui frame rendering the central panel
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.terminal_area(ui);
            });
        });

        // Verify that the legacy chat composer panel ("composer_panel") is never created in central view
        let composer_id = egui::Id::new("composer_panel");
        assert!(
            !ctx.memory(|m| m.has_focus(composer_id)),
            "chat composer panel should not be created or focused in central view"
        );
    }

    #[test]
    fn unconfigured_session_without_folder_shows_guidance_and_does_not_spawn_terminal() {
        let mut state = SavedState::default();
        // A session created without a folder (empty project_dir)
        let session = Session::new(20, PathBuf::new(), Provider::Claude, PermissionMode::ReadOnly);
        state.sessions = vec![session];
        state.active_session = 20;
        state.next_session_id = 21;

        let mut app = ViperApp::test_app(state);
        assert!(!app.active_session().has_folder(), "session starts without a project folder");
        assert_eq!(
            app.central_state(),
            CentralState::NeedsFolder,
            "central state identifies unconfigured session needing a folder"
        );

        // Render a headless egui pass
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.terminal_area(ui);
            });
        });

        // Verify that no terminal process was spawned into provider_terminals
        assert!(
            app.provider_terminals.is_empty(),
            "unconfigured session must not attempt to spawn a terminal process in provider_terminals"
        );
        assert!(
            !app.provider_terminals.contains_key(&20),
            "session 20 must have no terminal entry"
        );
    }

    #[test]
    fn session_with_missing_executable_displays_warning_without_panicking() {
        let mut state = SavedState::default();
        let session = Session::new(30, PathBuf::from(r"C:\work\project"), Provider::Codex, PermissionMode::ReadOnly);
        state.sessions = vec![session];
        state.active_session = 30;
        state.next_session_id = 31;

        let mut app = ViperApp::test_app(state);
        // Ensure no executable is detected for Codex
        app.detected.codex = None;
        assert!(app.detected.get(Provider::Codex).is_none(), "Codex executable is not detected");

        assert_eq!(
            app.central_state(),
            CentralState::MissingCli(Provider::Codex),
            "central state reports missing executable for Codex"
        );

        // Render headless egui pass: must execute cleanly without panicking
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.terminal_area(ui);
            });
        });

        // Verify that no terminal process was spawned into provider_terminals
        assert!(
            app.provider_terminals.is_empty(),
            "missing executable must not attempt terminal spawn in provider_terminals"
        );
    }

    #[test]
    fn switching_sessions_primes_keyboard_focus_flag_and_drawing_consumes_it() {
        let mut state = SavedState::default();
        let s1 = Session::new(41, PathBuf::from(r"C:\work\alpha"), Provider::Claude, PermissionMode::ReadOnly);
        let s2 = Session::new(42, PathBuf::from(r"C:\work\beta"), Provider::Codex, PermissionMode::Plan);
        state.sessions = vec![s1, s2];
        state.active_session = 41;
        state.next_session_id = 43;

        let mut app = ViperApp::test_app(state);
        let ctx = egui::Context::default();

        let [session_1, session_2] = &mut app.state.sessions[..] else {
            panic!("expected two sessions in test app")
        };
        // Reset initial focus flags to simulate steady state
        session_1.focus_composer = false;
        session_2.focus_composer = false;

        // Switch to session 42 via sidebar action
        app.handle_sidebar(SidebarAction::Select(42), &ctx);

        assert_eq!(app.state.active_session, 42, "sidebar select updates active session to 42");
        assert!(
            app.state.sessions[1].focus_composer,
            "switching to session 42 primes its keyboard focus flag"
        );
        assert!(
            !app.state.sessions[0].focus_composer,
            "inactive session 41 does not request keyboard focus"
        );

        // Rendering terminal_area consumes the focus flag
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.terminal_area(ui);
            });
        });

        assert!(
            !app.state.sessions[1].focus_composer,
            "drawing the terminal hands the focus flag over and resets it to false"
        );

        // Subsequent frame without session switch keeps focus flag false
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.terminal_area(ui);
            });
        });

        assert!(
            !app.state.sessions[1].focus_composer,
            "subsequent drawing frame does not re-prime focus flag"
        );
    }

    #[test]
    fn new_session_initializes_with_keyboard_focus_requested() {
        let state = SavedState::default();
        let mut app = ViperApp::test_app(state);
        let ctx = egui::Context::default();

        // Create a new session via sidebar
        app.handle_sidebar(SidebarAction::NewSession, &ctx);

        let active = app.active_session();
        assert!(
            active.focus_composer,
            "newly created session initializes with keyboard focus requested"
        );
        assert!(
            !active.has_folder(),
            "new session starts without a project folder"
        );
    }

    #[test]
    fn terminal_area_spawns_process_when_session_and_cli_are_ready() {
        let temp = std::env::temp_dir();
        let mut state = SavedState::default();
        let session = Session::new(50, temp.clone(), Provider::Claude, PermissionMode::ReadOnly);
        state.sessions = vec![session];
        state.active_session = 50;
        state.next_session_id = 51;

        let mut app = ViperApp::test_app(state);
        // Provide an existing executable path so detection succeeds
        let exe = if cfg!(windows) {
            PathBuf::from("cmd.exe")
        } else {
            PathBuf::from("/bin/sh")
        };
        app.detected.claude = Some(exe.clone());

        assert_eq!(
            app.central_state(),
            CentralState::TerminalReady,
            "session with folder and detected executable is ready for terminal"
        );

        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                app.terminal_area(ui);
            });
        });

        assert!(
            app.provider_terminals.contains_key(&50),
            "ready session spawns and registers a terminal in provider_terminals"
        );

        // Verify focus flag was consumed
        assert!(
            !app.state.sessions[0].focus_composer,
            "focus flag was transferred to the newly spawned terminal"
        );
    }
}
