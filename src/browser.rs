//! A browser panel: the system WebView placed over part of the app window.

use eframe::egui;

pub struct Browser {
    address: String,
    #[cfg(any(windows, target_os = "macos"))]
    native: Option<Result<native::NativeBrowser, String>>,
}

impl Browser {
    pub fn new(address: String) -> Self {
        Self {
            address,
            #[cfg(any(windows, target_os = "macos"))]
            native: None,
        }
    }

    /// The page to reopen next time the app starts.
    pub fn address(&self) -> &str {
        &self.address
    }

    #[cfg(any(windows, target_os = "macos"))]
    pub fn ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let mut action = None;
        let mut address_focused = false;
        ui.horizontal(|ui| {
            if ui.button("<").on_hover_text("Back").clicked() {
                action = Some(native::Action::Back);
            }
            if ui.button(">").on_hover_text("Forward").clicked() {
                action = Some(native::Action::Forward);
            }
            if ui.button("Reload").clicked() {
                action = Some(native::Action::Reload);
            }
            let go_clicked = ui.button("Go").clicked();
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.address)
                    .desired_width(f32::INFINITY)
                    .hint_text("Enter a URL, e.g. localhost:3000"),
            );
            address_focused = field.has_focus();
            let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if submitted || go_clicked {
                self.address = normalize_url(&self.address);
                action = Some(native::Action::Load(self.address.clone()));
            }
        });

        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, egui::Sense::hover());

        let native = self
            .native
            .get_or_insert_with(|| native::NativeBrowser::create(ui.ctx(), frame, &normalize_url(&self.address)));
        match native {
            Ok(browser) => {
                browser.place(rect, ui.ctx().pixels_per_point());
                if let Some(action) = action {
                    browser.run(action);
                }
                // Follow links clicked inside the page, unless the user is typing an address.
                if !address_focused && let Some(url) = browser.url() {
                    self.address = url;
                }
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.as_str());
            }
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &eframe::Frame) {
        ui.label("The built-in browser isn't available on this platform yet.");
    }

    /// Hides the page when the browser tab isn't showing. The page is a separate
    /// native window, so egui can't hide it by just not drawing it.
    pub fn hide(&mut self) {
        #[cfg(any(windows, target_os = "macos"))]
        if let Some(Ok(browser)) = &mut self.native {
            browser.hide();
        }
    }

    /// Gives keyboard focus back to the app after the user clicks outside the page.
    pub fn release_focus_on_click(&self, ctx: &egui::Context) {
        #[cfg(any(windows, target_os = "macos"))]
        if let Some(Ok(browser)) = &self.native
            && ctx.input(|i| i.pointer.any_pressed())
        {
            // Clicks inside the page never reach egui, so any click egui sees is outside it.
            browser.focus_app();
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        let _ = ctx;
    }
}

/// Turns what the user typed into a URL the WebView can load.
fn normalize_url(input: &str) -> String {
    let input = input.trim();
    if input.is_empty() {
        "about:blank".to_owned()
    } else if input.contains("://") || input.starts_with("about:") {
        input.to_owned()
    } else if input.starts_with("localhost") || input.starts_with("127.0.0.1") || input.starts_with("[::1]") {
        format!("http://{input}")
    } else {
        format!("https://{input}")
    }
}

#[cfg(any(windows, target_os = "macos"))]
mod native {
    use eframe::egui;
    use wry::dpi::{PhysicalPosition, PhysicalSize};
    use wry::{WebContext, WebView, WebViewBuilder};

    pub enum Action {
        Back,
        Forward,
        Reload,
        Load(String),
    }

    pub struct NativeBrowser {
        webview: WebView,
        // Kept alive for as long as the WebView uses it.
        _context: WebContext,
        bounds: Option<wry::Rect>,
        visible: bool,
    }

    impl NativeBrowser {
        pub fn create(ctx: &egui::Context, frame: &eframe::Frame, url: &str) -> Result<Self, String> {
            let window = frame.winit_window().ok_or("The app window isn't ready for a browser yet.")?;
            let data_dir = eframe::storage_dir("Barduino").map(|dir| dir.join("webview"));
            let mut context = WebContext::new(data_dir);

            let repaint = ctx.clone();
            let repaint_on_load = ctx.clone();
            let webview = WebViewBuilder::new_with_web_context(&mut context)
                .with_url(url)
                .with_visible(false)
                // Redraw the app so the address bar picks up the new page.
                .with_navigation_handler(move |_| {
                    repaint.request_repaint();
                    true
                })
                .with_on_page_load_handler(move |_, _| repaint_on_load.request_repaint())
                .build_as_child(window.as_ref())
                .map_err(|err| format!("Couldn't start the browser: {err}"))?;

            Ok(Self { webview, _context: context, bounds: None, visible: false })
        }

        /// Moves the page over `rect` (in egui points) and shows it.
        pub fn place(&mut self, rect: egui::Rect, pixels_per_point: f32) {
            let bounds = wry::Rect {
                position: PhysicalPosition::new(
                    (rect.min.x * pixels_per_point).round() as i32,
                    (rect.min.y * pixels_per_point).round() as i32,
                )
                .into(),
                size: PhysicalSize::new(
                    (rect.width() * pixels_per_point).round().max(1.0) as u32,
                    (rect.height() * pixels_per_point).round().max(1.0) as u32,
                )
                .into(),
            };
            if self.bounds != Some(bounds) {
                let _ = self.webview.set_bounds(bounds);
                self.bounds = Some(bounds);
            }
            if !self.visible {
                let _ = self.webview.set_visible(true);
                self.visible = true;
            }
        }

        pub fn hide(&mut self) {
            if self.visible {
                let _ = self.webview.set_visible(false);
                let _ = self.webview.focus_parent();
                self.visible = false;
            }
        }

        pub fn focus_app(&self) {
            let _ = self.webview.focus_parent();
        }

        pub fn url(&self) -> Option<String> {
            self.webview.url().ok().filter(|url| !url.is_empty())
        }

        pub fn run(&self, action: Action) {
            let _ = match action {
                Action::Back => self.webview.evaluate_script("history.back()"),
                Action::Forward => self.webview.evaluate_script("history.forward()"),
                Action::Reload => self.webview.reload(),
                Action::Load(url) => self.webview.load_url(&url),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_url;

    #[test]
    fn normalizes_typed_addresses() {
        assert_eq!(normalize_url(""), "about:blank");
        assert_eq!(normalize_url("localhost:3000"), "http://localhost:3000");
        assert_eq!(normalize_url("127.0.0.1:8080/app"), "http://127.0.0.1:8080/app");
        assert_eq!(normalize_url("docs.rs"), "https://docs.rs");
        assert_eq!(normalize_url(" http://example.com "), "http://example.com");
        assert_eq!(normalize_url("about:blank"), "about:blank");
    }
}
