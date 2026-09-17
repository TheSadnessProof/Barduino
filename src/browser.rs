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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PickedElement {
    pub url: String,
    pub selector: String,
    pub tag: String,
    pub text: String,
    pub html: String,
    pub width: u32,
    pub height: u32,
    /// What the user wants done here, when the element came from a comment.
    #[serde(default)]
    pub note: Option<String>,
}

impl PickedElement {
    /// A short name for the element, like `button.primary`.
    pub fn short_label(&self) -> String {
        shorten(&self.tag, 32)
    }

    /// The start of the element's text, if it has any.
    pub fn short_text(&self) -> Option<String> {
        let text = self.text.split_whitespace().collect::<Vec<_>>().join(" ");
        (!text.is_empty()).then(|| shorten(&text, 28))
    }

    /// The note written about this element, if it came from a comment.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref().map(str::trim).filter(|note| !note.is_empty())
    }

    /// How the element is described to the agent when the message is sent.
    pub fn as_prompt(&self) -> String {
        let heading = match self.note() {
            Some(note) => format!("Comment on {}: {note}", self.url),
            None => format!("Element on {}", self.url),
        };
        let mut prompt = format!(
            "{heading}\nThe element is {} × {} px.\nSelector: `{}`\n```html\n{}\n```",
            self.width, self.height, self.selector, self.html
        );
        if self.html.chars().count() >= 3000 {
            prompt.push_str("\n(HTML cut off)");
        }
        prompt
    }
}

/// A comment the user left on an element, numbered to match its pin on the page.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Comment {
    pub number: u32,
    #[serde(flatten)]
    pub element: PickedElement,
    /// What the user typed about it, which starts empty.
    #[serde(skip)]
    pub note: String,
}

/// A message sent from the page through `window.ipc`.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum PageMessage {
    Picked(PickedElement),
    Commented(Comment),
    Cancelled,
}

/// What clicking on the page does at the moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Off,
    /// One click sends an element to the message box.
    Picking,
    /// Each click pins a numbered comment on the page.
    Commenting,
}

pub enum BrowserAction {
    None,
    /// Attach these elements to the message being written.
    Attach(Vec<PickedElement>),
    /// Send these elements directly to the active session.
    Send(Vec<PickedElement>),
}

pub struct Browser {
    pub state: BrowserState,
    mode: Mode,
    /// Comments waiting to be sent, in the order they were left.
    comments: Vec<Comment>,
    /// The comment whose note should take the keyboard next time it's drawn.
    focus_note: Option<u32>,
    #[cfg(any(windows, target_os = "macos"))]
    native: Option<Result<native::NativeBrowser, String>>,
}

impl Browser {
    pub fn new(state: BrowserState) -> Self {
        Self {
            state,
            mode: Mode::Off,
            comments: Vec::new(),
            focus_note: None,
            #[cfg(any(windows, target_os = "macos"))]
            native: None,
        }
    }

    /// Closes the page. It opens again the next time the browser tab is shown.
    pub fn close(&mut self) {
        self.mode = Mode::Off;
        self.comments.clear();
        self.focus_note = None;
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
                    // Only accept what the user started, not what a page sends on its own.
                    native::Message::Page(PageMessage::Picked(element)) if self.mode == Mode::Picking => {
                        self.mode = Mode::Off;
                        result = BrowserAction::Attach(vec![element]);
                    }
                    native::Message::Page(PageMessage::Commented(comment))
                        if self.mode == Mode::Commenting =>
                    {
                        self.focus_note = Some(comment.number);
                        self.comments.push(comment);
                    }
                    native::Message::Page(PageMessage::Cancelled) => self.mode = Mode::Off,
                    native::Message::Page(_) => {}
                    // A new page starts without picking or commenting, and its pins are gone
                    // with it. The comments themselves are kept, since the notes are the point.
                    native::Message::PageLoading => self.mode = Mode::Off,
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
                let commenting = self.mode == Mode::Commenting;
                let comment_tip = if commenting {
                    "Commenting: click the places you want changed (Esc to stop)"
                } else {
                    "Leave comments on the page, then send them to the agent"
                };
                if icons::toggle(ui, Icon::Comment, comment_tip, commenting).clicked() {
                    self.mode = if commenting { Mode::Off } else { Mode::Commenting };
                    // Numbering carries on from the comments already listed, so the pins
                    // on the page match the list even after a reload.
                    let from = self.comments.iter().map(|comment| comment.number).max().unwrap_or(0) + 1;
                    commands.push(Command::Comment(self.mode == Mode::Commenting, from));
                }

                let picking = self.mode == Mode::Picking;
                let pick_tip = if picking {
                    "Picking: click an element on the page (Esc to cancel)"
                } else {
                    "Select an element on the page"
                };
                if icons::toggle(ui, Icon::Pick, pick_tip, picking).clicked() {
                    self.mode = if picking { Mode::Off } else { Mode::Picking };
                    commands.push(Command::Pick(self.mode == Mode::Picking));
                }

                match self.mode {
                    Mode::Picking => {
                        ui.label(egui::RichText::new("Click an element").small().weak());
                    }
                    Mode::Commenting => {
                        ui.label(egui::RichText::new("Click a place to comment on").small().weak());
                    }
                    Mode::Off => {}
                }
            });
        });

        if !self.comments.is_empty() {
            ui.add_space(4.0);
            if let Some(action) = self.comment_list(ui, &mut commands) {
                result = action;
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

    /// The comments left so far: a note each, and buttons to send or drop them.
    #[cfg(any(windows, target_os = "macos"))]
    fn comment_list(&mut self, ui: &mut egui::Ui, commands: &mut Vec<native::Command>) -> Option<BrowserAction> {
        let mut action = None;
        let mut remove = None;
        let mut send_single = None;
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .corner_radius(8.0)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let count = self.comments.len();
                ui.horizontal(|ui| {
                    let heading = if count == 1 { "1 comment".to_owned() } else { format!("{count} comments") };
                    ui.label(egui::RichText::new(heading).strong().small());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let send_direct = ui
                            .button(egui::RichText::new("Send directly").small().strong())
                            .on_hover_text("Sends all comments directly to the session");
                        if send_direct.clicked() {
                            let elements = self.comments.drain(..).map(Comment::into_element).collect();
                            commands.push(native::Command::ClearPins);
                            self.mode = Mode::Off;
                            commands.push(native::Command::Comment(false, 1));
                            action = Some(BrowserAction::Send(elements));
                        }
                        let add_label = if count == 1 { "Add comment" } else { "Add comments" };
                        let add_btn = ui
                            .button(egui::RichText::new(add_label).small())
                            .on_hover_text("Accumulates comments in the message box to send together later");
                        if add_btn.clicked() {
                            let elements = self.comments.drain(..).map(Comment::into_element).collect();
                            commands.push(native::Command::ClearPins);
                            self.mode = Mode::Off;
                            commands.push(native::Command::Comment(false, 1));
                            action = Some(BrowserAction::Attach(elements));
                        }
                        if ui.button(egui::RichText::new("Clear").small()).clicked() {
                            self.comments.clear();
                            commands.push(native::Command::ClearPins);
                        }
                    });
                });
                ui.add_space(2.0);

                egui::ScrollArea::vertical().max_height(150.0).id_salt("comments").show(ui, |ui| {
                    for comment in &mut self.comments {
                        ui.horizontal(|ui| {
                            pin_number(ui, comment.number);
                            let note = ui.add(
                                egui::TextEdit::singleline(&mut comment.note)
                                    .desired_width((ui.available_width() - 170.0).max(60.0))
                                    .hint_text("What should change here?"),
                            );
                            // The note for a fresh pin takes the keyboard, so the user can just type.
                            if self.focus_note == Some(comment.number) {
                                note.request_focus();
                                self.focus_note = None;
                            }
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(comment.element.short_label()).monospace().small().weak(),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&comment.element.selector);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if icons::button(ui, Icon::Close, "Remove this comment").clicked() {
                                    remove = Some(comment.number);
                                }
                                if ui
                                    .small_button("Send")
                                    .on_hover_text("Send this comment directly to the session")
                                    .clicked()
                                {
                                    send_single = Some(comment.number);
                                }
                            });
                        });
                    }
                });
            });

        if let Some(number) = send_single
            && let Some(pos) = self.comments.iter().position(|c| c.number == number)
        {
            let comment = self.comments.remove(pos);
            commands.push(native::Command::RemovePin(number));
            if self.comments.is_empty() {
                self.mode = Mode::Off;
                commands.push(native::Command::Comment(false, 1));
            }
            action = Some(BrowserAction::Send(vec![comment.into_element()]));
        }
        if let Some(number) = remove {
            self.comments.retain(|comment| comment.number != number);
            commands.push(native::Command::RemovePin(number));
        }
        action
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

impl Comment {
    /// The element with the note attached, which is what the message carries.
    fn into_element(self) -> PickedElement {
        let note = self.note.trim();
        PickedElement { note: (!note.is_empty()).then(|| note.to_owned()), ..self.element }
    }
}

/// The number that matches a comment to its pin on the page.
fn pin_number(ui: &mut egui::Ui, number: u32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    // The same orange as the pin the page draws.
    const PIN: egui::Color32 = egui::Color32::from_rgb(217, 119, 63);
    ui.painter().circle_filled(rect.center(), 10.0, PIN);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        number.to_string(),
        egui::FontId::proportional(12.0),
        egui::Color32::WHITE,
    );
}

/// Cuts `text` to at most `max` characters, ending with "…" if anything was cut.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut short: String = text.chars().take(max - 1).collect();
    short.push('…');
    short
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
        /// Turn commenting on or off, numbering new pins from this number.
        Comment(bool, u32),
        RemovePin(u32),
        ClearPins,
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
                Command::Comment(on, from) => {
                    if on {
                        let _ = self.webview.focus();
                    }
                    self.webview
                        .evaluate_script(&format!("window.__barduino && window.__barduino.comment({on}, {from})"))
                }
                Command::RemovePin(number) => self
                    .webview
                    .evaluate_script(&format!("window.__barduino && window.__barduino.removePin({number})")),
                Command::ClearPins => {
                    self.webview.evaluate_script("window.__barduino && window.__barduino.clearPins()")
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
        assert_eq!(element.short_label(), "button.primary");
        assert_eq!(element.short_text().as_deref(), Some("Sign in"));
        // A comment is the same element plus the number of its pin on the page.
        let commented = r#"{"kind":"commented","number":2,"url":"http://localhost:3000/","selector":"main > h1","tag":"h1","text":"Welcome","html":"<h1>Welcome</h1>","width":400,"height":48}"#;
        let Ok(PageMessage::Commented(comment)) = serde_json::from_str::<PageMessage>(commented) else {
            panic!("should parse a commented element");
        };
        assert_eq!((comment.number, comment.element.tag.as_str()), (2, "h1"));
        assert_eq!(comment.note, "", "the note is typed in the app, not on the page");

        let noted = Comment { note: "  Make this bigger  ".into(), ..comment }.into_element();
        assert_eq!(noted.note(), Some("Make this bigger"), "the note is trimmed onto the element");
        let prompt = noted.as_prompt();
        assert!(prompt.starts_with("Comment on http://localhost:3000/: Make this bigger"), "{prompt}");
        assert!(prompt.contains("Selector: `main > h1`"), "{prompt}");
        // A note that is only spaces leaves an ordinary picked element.
        let blank = Comment { number: 3, element: noted.clone(), note: "   ".into() }.into_element();
        assert_eq!(blank.note(), None);

        assert_eq!(serde_json::from_str::<PageMessage>(r#"{"kind":"cancelled"}"#).unwrap(), PageMessage::Cancelled);
        assert!(serde_json::from_str::<PageMessage>(r#"{"kind":"steal-cookies"}"#).is_err());
    }
}
