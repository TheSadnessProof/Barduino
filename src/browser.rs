//! A browser panel: the system WebView placed over part of the app window, with
//! screen-size presets and a picker for choosing elements on the page.

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::icons::{self, Icon};

/// The page size to show the site at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Viewport {
    /// Fill the panel.
    #[default]
    Desktop,
    Tablet,
    Mobile,
    Custom,
}

impl Viewport {
    const ALL: [Viewport; 4] = [Self::Desktop, Self::Tablet, Self::Mobile, Self::Custom];

    fn label(self) -> &'static str {
        match self {
            Self::Desktop => "Desktop",
            Self::Tablet => "Tablet",
            Self::Mobile => "Mobile",
            Self::Custom => "Custom",
        }
    }
}

/// Browser choices that are saved between launches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserState {
    pub address: String,
    pub viewport: Viewport,
    /// Width and height used by the Custom viewport, in CSS pixels.
    pub custom_size: [u32; 2],
}

impl Default for BrowserState {
    fn default() -> Self {
        Self { address: String::new(), viewport: Viewport::Desktop, custom_size: [1280, 800] }
    }
}

/// An element the user picked on the page.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PickedElement {
    pub url: String,
    pub selector: String,
    pub tag: String,
    pub text: String,
    pub html: String,
    pub width: u32,
    pub height: u32,
}

impl PickedElement {
    /// How the element is described when it's added to a message for the agent.
    pub fn as_prompt(&self) -> String {
        let mut prompt = format!(
            "Element on {} ({} × {} px)\nSelector: `{}`\n```html\n{}\n```",
            self.url, self.width, self.height, self.selector, self.html
        );
        if self.html.chars().count() >= 3000 {
            prompt.push_str("\n(HTML cut off)");
        }
        prompt
    }
}

/// A message sent from the page through `window.ipc`.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum PageMessage {
    Picked(PickedElement),
    Cancelled,
}

pub enum BrowserAction {
    None,
    /// Add this text to the message box.
    AddToMessage(String),
}

pub struct Browser {
    pub state: BrowserState,
    picking: bool,
    picked: Option<PickedElement>,
    #[cfg(any(windows, target_os = "macos"))]
    native: Option<Result<native::NativeBrowser, String>>,
}

impl Browser {
    pub fn new(state: BrowserState) -> Self {
        Self {
            state,
            picking: false,
            picked: None,
            #[cfg(any(windows, target_os = "macos"))]
            native: None,
        }
    }

    /// Closes the page. It opens again the next time the browser tab is shown.
    pub fn close(&mut self) {
        self.picking = false;
        self.picked = None;
        #[cfg(any(windows, target_os = "macos"))]
        {
            self.native = None;
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn ui(&mut self, ui: &mut egui::Ui, _frame: &eframe::Frame, _page_visible: bool) -> BrowserAction {
        ui.label("The built-in browser isn't available on this platform yet.");
        BrowserAction::None
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn hide(&mut self) {}

    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn release_focus_on_click(&self, _ctx: &egui::Context) {}

    /// Draws the browser. `page_visible` is false while something (such as an
    /// open menu) needs to draw over the area where the page would be.
    #[cfg(any(windows, target_os = "macos"))]
    pub fn ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, page_visible: bool) -> BrowserAction {
        use native::Command;

        let mut commands = Vec::new();
        let mut result = BrowserAction::None;

        // Messages from the page arrive between frames.
        if let Some(Ok(browser)) = &self.native {
            for message in browser.take_messages() {
                match message {
                    // Only accept a pick the user started, not one a page sends on its own.
                    native::Message::Page(PageMessage::Picked(element)) if self.picking => {
                        self.picking = false;
                        self.picked = Some(element);
                    }
                    native::Message::Page(PageMessage::Cancelled) => self.picking = false,
                    native::Message::Page(PageMessage::Picked(_)) => {}
                    // A new page starts without the picker running.
                    native::Message::PageLoading => self.picking = false,
                }
            }
        }

        let mut address_focused = false;
        ui.horizontal(|ui| {
            if ui.button("<").on_hover_text("Back").clicked() {
                commands.push(Command::Back);
            }
            if ui.button(">").on_hover_text("Forward").clicked() {
                commands.push(Command::Forward);
            }
            if ui.button("⟳").on_hover_text("Reload").clicked() {
                commands.push(Command::Reload);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let go = ui.button("Go").clicked();
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.state.address)
                        .desired_width(f32::INFINITY)
                        .hint_text("Enter a URL, e.g. localhost:3000"),
                );
                address_focused = field.has_focus();
                let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if submitted || go {
                    self.state.address = normalize_url(&self.state.address);
                    commands.push(Command::Load(self.state.address.clone()));
                }
            });
        });

        ui.horizontal(|ui| {
            for viewport in Viewport::ALL {
                ui.selectable_value(&mut self.state.viewport, viewport, viewport.label());
            }
            if self.state.viewport == Viewport::Custom {
                let [width, height] = &mut self.state.custom_size;
                ui.add(egui::DragValue::new(width).range(200..=3840).suffix(" px"));
                ui.label("×");
                ui.add(egui::DragValue::new(height).range(200..=2400).suffix(" px"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let tooltip = if self.picking {
                    "Picking: click an element on the page (Esc to cancel)"
                } else {
                    "Select an element on the page"
                };
                if icons::toggle(ui, Icon::Pick, tooltip, self.picking).clicked() {
                    self.picking = !self.picking;
                    commands.push(Command::Pick(self.picking));
                }
                if self.picking {
                    ui.label(egui::RichText::new("Click an element").small().weak());
                }
            });
        });

        if let Some(element) = self.picked.clone() {
            match picked_card(ui, &element) {
                CardAction::None => {}
                CardAction::Add => {
                    result = BrowserAction::AddToMessage(element.as_prompt());
                    self.picked = None;
                }
                CardAction::Dismiss => self.picked = None,
            }
        }

        let area = ui.available_rect_before_wrap();
        ui.allocate_rect(area, egui::Sense::hover());
        let (page, zoom) = page_rect(area, self.state.viewport, self.state.custom_size);
        if self.state.viewport != Viewport::Desktop {
            ui.painter().rect_filled(area, 0.0, ui.visuals().extreme_bg_color);
            let [width, height] = viewport_size(self.state.viewport, self.state.custom_size);
            ui.painter().text(
                egui::pos2(area.center().x, page.bottom() + 12.0),
                egui::Align2::CENTER_CENTER,
                format!("{width} × {height} · {:.0}%", zoom * 100.0),
                egui::FontId::proportional(11.0),
                ui.visuals().weak_text_color(),
            );
        }

        let url = normalize_url(&self.state.address);
        let native = self.native.get_or_insert_with(|| native::NativeBrowser::create(ui.ctx(), frame, &url));
        match native {
            Ok(browser) => {
                if page_visible {
                    browser.place(page, zoom, ui.ctx().pixels_per_point());
                } else {
                    browser.hide();
                }
                for command in commands {
                    browser.run(command);
                }
                // Follow links clicked inside the page, unless the user is typing an address.
                if !address_focused && let Some(url) = browser.url() {
                    self.state.address = url;
                }
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.as_str());
            }
        }
        result
    }

    /// Hides the page when the browser tab isn't showing. The page is a separate
    /// native window, so egui can't hide it by just not drawing it.
    #[cfg(any(windows, target_os = "macos"))]
    pub fn hide(&mut self) {
        if let Some(Ok(browser)) = &mut self.native {
            browser.hide();
        }
    }

    /// Gives keyboard focus back to the app after the user clicks outside the page.
    #[cfg(any(windows, target_os = "macos"))]
    pub fn release_focus_on_click(&self, ctx: &egui::Context) {
        if let Some(Ok(browser)) = &self.native
            && ctx.input(|i| i.pointer.any_pressed())
        {
            // Clicks inside the page never reach egui, so any click egui sees is outside it.
            browser.focus_app();
        }
    }
}

enum CardAction {
    None,
    Add,
    Dismiss,
}

fn picked_card(ui: &mut egui::Ui, element: &PickedElement) -> CardAction {
    let mut action = CardAction::None;
    egui::Frame::group(ui.style()).fill(ui.visuals().faint_bg_color).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&element.tag).monospace().strong());
            ui.label(egui::RichText::new(format!("{} × {}", element.width, element.height)).small().weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::button(ui, Icon::Close, "Dismiss").clicked() {
                    action = CardAction::Dismiss;
                }
                if ui.button("Copy selector").clicked() {
                    ui.ctx().copy_text(element.selector.clone());
                }
                if ui.button("Add to message").on_hover_text("Adds the selector and HTML to your message").clicked() {
                    action = CardAction::Add;
                }
            });
        });
        ui.add(egui::Label::new(egui::RichText::new(&element.selector).monospace().small()).truncate());
        if !element.text.is_empty() {
            ui.add(egui::Label::new(egui::RichText::new(format!("“{}”", element.text)).small().weak()).truncate());
        }
    });
    action
}

/// The page size in CSS pixels, or the area's own size for Desktop.
fn viewport_size(viewport: Viewport, custom_size: [u32; 2]) -> [u32; 2] {
    match viewport {
        Viewport::Desktop => [0, 0],
        Viewport::Tablet => [768, 1024],
        Viewport::Mobile => [375, 812],
        Viewport::Custom => custom_size,
    }
}

/// Where to put the page inside `area`, and how far to zoom it out so a device
/// size that is bigger than the panel still fits while keeping its CSS width.
fn page_rect(area: egui::Rect, viewport: Viewport, custom_size: [u32; 2]) -> (egui::Rect, f32) {
    if viewport == Viewport::Desktop {
        return (area, 1.0);
    }
    const MARGIN: f32 = 12.0;
    const CAPTION: f32 = 24.0;
    let [width, height] = viewport_size(viewport, custom_size).map(|v| v.max(1) as f32);
    let room = egui::vec2((area.width() - 2.0 * MARGIN).max(50.0), (area.height() - MARGIN - CAPTION).max(50.0));
    let zoom = (room.x / width).min(room.y / height).min(1.0);
    let size = egui::vec2(width, height) * zoom;
    let rect = egui::Rect::from_min_size(egui::pos2(area.center().x - size.x / 2.0, area.top() + MARGIN), size);
    (rect, zoom)
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
    use std::cell::RefCell;
    use std::rc::Rc;

    use eframe::egui;
    use wry::dpi::{PhysicalPosition, PhysicalSize};
    use wry::{PageLoadEvent, WebContext, WebView, WebViewBuilder};

    use super::PageMessage;

    pub enum Command {
        Back,
        Forward,
        Reload,
        Load(String),
        Pick(bool),
    }

    pub enum Message {
        Page(PageMessage),
        PageLoading,
    }

    pub struct NativeBrowser {
        webview: WebView,
        // Kept alive for as long as the WebView uses it.
        _context: WebContext,
        messages: Rc<RefCell<Vec<Message>>>,
        bounds: Option<wry::Rect>,
        zoom: f32,
        visible: bool,
    }

    impl NativeBrowser {
        pub fn create(ctx: &egui::Context, frame: &eframe::Frame, url: &str) -> Result<Self, String> {
            let window = frame.winit_window().ok_or("The app window isn't ready for a browser yet.")?;
            let data_dir = eframe::storage_dir("Barduino").map(|dir| dir.join("webview"));
            let mut context = WebContext::new(data_dir);
            let messages = Rc::new(RefCell::new(Vec::new()));

            let (repaint, ipc_repaint, load_repaint) = (ctx.clone(), ctx.clone(), ctx.clone());
            let (ipc_messages, load_messages) = (Rc::clone(&messages), Rc::clone(&messages));
            let webview = WebViewBuilder::new_with_web_context(&mut context)
                .with_url(url)
                .with_visible(false)
                .with_initialization_script(include_str!("picker.js"))
                .with_ipc_handler(move |request| {
                    // Anything that isn't one of our messages is ignored.
                    if let Ok(message) = serde_json::from_str::<PageMessage>(request.body()) {
                        ipc_messages.borrow_mut().push(Message::Page(message));
                        ipc_repaint.request_repaint();
                    }
                })
                // Redraw the app so the address bar picks up the new page.
                .with_navigation_handler(move |_| {
                    repaint.request_repaint();
                    true
                })
                .with_on_page_load_handler(move |event, _| {
                    if let PageLoadEvent::Started = event {
                        load_messages.borrow_mut().push(Message::PageLoading);
                    }
                    load_repaint.request_repaint();
                })
                .build_as_child(window.as_ref())
                .map_err(|err| format!("Couldn't start the browser: {err}"))?;

            Ok(Self { webview, _context: context, messages, bounds: None, zoom: 1.0, visible: false })
        }

        pub fn take_messages(&self) -> Vec<Message> {
            std::mem::take(&mut *self.messages.borrow_mut())
        }

        /// Moves the page over `rect` (in egui points), zooms it, and shows it.
        pub fn place(&mut self, rect: egui::Rect, zoom: f32, pixels_per_point: f32) {
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
            if (self.zoom - zoom).abs() > 0.001 {
                let _ = self.webview.zoom(zoom as f64);
                self.zoom = zoom;
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

        pub fn run(&self, command: Command) {
            let _ = match command {
                Command::Back => self.webview.evaluate_script("history.back()"),
                Command::Forward => self.webview.evaluate_script("history.forward()"),
                Command::Reload => self.webview.reload(),
                Command::Load(url) => self.webview.load_url(&url),
                Command::Pick(on) => {
                    // Picking needs keyboard focus on the page so Esc can cancel it.
                    if on {
                        let _ = self.webview.focus();
                    }
                    self.webview.evaluate_script(&format!("window.__barduino && window.__barduino.pick({on})"))
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_typed_addresses() {
        assert_eq!(normalize_url(""), "about:blank");
        assert_eq!(normalize_url("localhost:3000"), "http://localhost:3000");
        assert_eq!(normalize_url("127.0.0.1:8080/app"), "http://127.0.0.1:8080/app");
        assert_eq!(normalize_url("docs.rs"), "https://docs.rs");
        assert_eq!(normalize_url(" http://example.com "), "http://example.com");
        assert_eq!(normalize_url("about:blank"), "about:blank");
    }

    #[test]
    fn device_sizes_shrink_to_fit_but_never_grow() {
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(500.0, 600.0));
        let (desktop, zoom) = page_rect(area, Viewport::Desktop, [0, 0]);
        assert_eq!((desktop, zoom), (area, 1.0));

        let (mobile, zoom) = page_rect(area, Viewport::Mobile, [0, 0]);
        assert!(zoom < 1.0, "812 px tall doesn't fit in 600");
        assert!((mobile.width() / mobile.height() - 375.0 / 812.0).abs() < 0.001, "keeps the phone's shape");
        assert!(area.contains_rect(mobile));

        let big = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(2000.0, 2000.0));
        let (tablet, zoom) = page_rect(big, Viewport::Tablet, [0, 0]);
        assert_eq!(zoom, 1.0);
        assert_eq!(tablet.size(), egui::vec2(768.0, 1024.0));
    }

    #[test]
    fn page_messages_parse() {
        let picked = r#"{"kind":"picked","url":"http://localhost:3000/","selector":"main > button.primary","tag":"button.primary","text":"Sign in","html":"<button class=\"primary\">Sign in</button>","width":120,"height":36}"#;
        let Ok(PageMessage::Picked(element)) = serde_json::from_str::<PageMessage>(picked) else {
            panic!("should parse a picked element");
        };
        assert_eq!(element.selector, "main > button.primary");
        assert!(element.as_prompt().contains("Selector: `main > button.primary`"));
        assert_eq!(serde_json::from_str::<PageMessage>(r#"{"kind":"cancelled"}"#).unwrap(), PageMessage::Cancelled);
        assert!(serde_json::from_str::<PageMessage>(r#"{"kind":"steal-cookies"}"#).is_err());
    }
}
