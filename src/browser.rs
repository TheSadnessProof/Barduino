//! A browser panel: the system WebView placed over part of the app window, with
//! screen-size presets and a picker for choosing elements on the page.

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::icons::{self, Icon};

/// The sizes the device buttons set, in CSS pixels.
pub const TABLET_SIZE: [u32; 2] = [768, 1024];
pub const MOBILE_SIZE: [u32; 2] = [375, 812];

/// The page size to show the site at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Viewport {
    /// Fill the panel.
    #[default]
    Desktop,
    /// The size in the pixel boxes, which the device buttons preset.
    Fixed,
}

/// Browser choices that are saved between launches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "SavedBrowserState")]
pub struct BrowserState {
    pub address: String,
    pub viewport: Viewport,
    /// The size the `Fixed` viewport shows the page at, in CSS pixels.
    pub size: [u32; 2],
}

impl Default for BrowserState {
    fn default() -> Self {
        Self { address: String::new(), viewport: Viewport::Desktop, size: [1280, 800] }
    }
}

/// Saves written before the device buttons replaced the Tablet, Mobile and Custom
/// presets still name them, and still call the size `custom_size`.
#[derive(Deserialize)]
#[serde(default)]
struct SavedBrowserState {
    address: String,
    viewport: SavedViewport,
    /// The size the old Custom preset used.
    custom_size: [u32; 2],
    /// The size the pixel boxes chose. Zeroes mean the save predates them, and
    /// zeroes rather than an `Option` because RON insists on `Some(…)` for those.
    size: [u32; 2],
}

impl Default for SavedBrowserState {
    fn default() -> Self {
        let BrowserState { address, size, .. } = BrowserState::default();
        Self { address, viewport: SavedViewport::Desktop, custom_size: size, size: [0, 0] }
    }
}

#[derive(Clone, Copy, Default, Deserialize)]
enum SavedViewport {
    #[default]
    Desktop,
    Tablet,
    Mobile,
    Custom,
    Fixed,
}

impl From<SavedBrowserState> for BrowserState {
    fn from(saved: SavedBrowserState) -> Self {
        let chosen = if saved.size == [0, 0] { saved.custom_size } else { saved.size };
        let (viewport, size) = match saved.viewport {
            SavedViewport::Desktop => (Viewport::Desktop, chosen),
            SavedViewport::Tablet => (Viewport::Fixed, TABLET_SIZE),
            SavedViewport::Mobile => (Viewport::Fixed, MOBILE_SIZE),
            SavedViewport::Custom | SavedViewport::Fixed => (Viewport::Fixed, chosen),
        };
        Self { address: saved.address, viewport, size }
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

/// The single system WebView, which every session takes turns showing. What each
/// session wants shown in it lives in its own [`BrowserState`].
pub struct Browser {
    mode: Mode,
    /// Comments waiting to be sent, in the order they were left.
    comments: Vec<Comment>,
    /// The comment whose note should take the keyboard next time it's drawn.
    focus_note: Option<u32>,
    /// The page's size last frame, so the pixel boxes can report what filling the
    /// panel actually works out to.
    page_size: [u32; 2],
    /// The address the WebView is actually showing, so a session whose own address
    /// differs from it is one whose page still has to be fetched.
    loaded: String,
    /// Which session's page is in the WebView. Comments and picking mode belong to
    /// that session, and the address alone can't tell them apart: two sessions in
    /// one project usually point at the same dev server.
    /// Which browser tab of which session the page belongs to. Marks left on it are
    /// that tab's, so switching to another tab — even one on the same address —
    /// starts clean rather than sending one page's comments about another.
    showing: Option<(u64, u64)>,
    #[cfg(any(windows, target_os = "macos"))]
    native: Option<Result<native::NativeBrowser, String>>,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            mode: Mode::Off,
            comments: Vec::new(),
            focus_note: None,
            page_size: BrowserState::default().size,
            loaded: String::new(),
            showing: None,
            #[cfg(any(windows, target_os = "macos"))]
            native: None,
        }
    }
}

impl Browser {
    /// Closes the page. It opens again the next time a browser tab is shown.
    pub fn close(&mut self) {
        self.forget_marks();
        self.loaded.clear();
        self.showing = None;
        #[cfg(any(windows, target_os = "macos"))]
        {
            self.native = None;
        }
    }

    /// Drops what was left on the page: the comments, the pins they go with, and
    /// whatever clicking was about to do.
    fn forget_marks(&mut self) {
        self.mode = Mode::Off;
        self.comments.clear();
        self.focus_note = None;
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn ui(
        &mut self,
        _owner: (u64, u64),
        _state: &mut BrowserState,
        ui: &mut egui::Ui,
        _frame: &eframe::Frame,
        _page_visible: bool,
    ) -> BrowserAction {
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
    pub fn ui(
        &mut self,
        owner: (u64, u64),
        state: &mut BrowserState,
        ui: &mut egui::Ui,
        frame: &eframe::Frame,
        page_visible: bool,
    ) -> BrowserAction {
        use native::Command;

        let mut commands = Vec::new();
        let mut result = BrowserAction::None;

        // Comments and picking belong to the tab they were left on, even when the
        // next one happens to point at the same address.
        if self.showing != Some(owner) {
            self.forget_marks();
            self.showing = Some(owner);
        }
        // The page itself only needs fetching again when the address really differs.
        let wanted = normalize_url(&state.address);
        if !self.loaded.is_empty() && self.loaded != wanted {
            commands.push(Command::Load(wanted));
        }

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
                    egui::TextEdit::singleline(&mut state.address)
                        .desired_width(f32::INFINITY)
                        .hint_text("Enter a URL, e.g. localhost:3000"),
                );
                address_focused = field.has_focus();
                let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if submitted || go {
                    state.address = normalize_url(&state.address);
                    commands.push(Command::Load(state.address.clone()));
                }
            });
        });

        ui.horizontal(|ui| {
            // Desktop means "fill the panel"; the other two are shortcuts for the
            // pixel boxes beside them, which are what actually decide the size.
            let fitting = state.viewport == Viewport::Desktop;
            if icons::toggle(ui, Icon::Desktop, "Fit the panel", fitting).clicked() {
                state.viewport = Viewport::Desktop;
            }
            for (icon, name, preset) in
                [(Icon::Tablet, "Tablet", TABLET_SIZE), (Icon::Mobile, "Mobile", MOBILE_SIZE)]
            {
                let [width, height] = preset;
                let tip = format!("{name} — {width} × {height}");
                let chosen = !fitting && state.size == preset;
                if icons::toggle(ui, icon, &tip, chosen).clicked() {
                    state.viewport = Viewport::Fixed;
                    state.size = preset;
                }
            }

            // While the page fills the panel the boxes report the size that comes out
            // of that, and typing in one switches to that size instead.
            let mut size = if fitting { self.page_size } else { state.size };
            let [width, height] = &mut size;
            let mut retyped = ui.add(egui::DragValue::new(width).range(200..=3840).suffix(" px")).changed();
            ui.label("×");
            retyped |= ui.add(egui::DragValue::new(height).range(200..=2400).suffix(" px")).changed();
            if retyped {
                state.viewport = Viewport::Fixed;
                state.size = size;
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
        let (page, zoom) = page_rect(area, state.viewport, state.size);
        if state.viewport == Viewport::Desktop {
            // Filling the panel has no size of its own, so remember what it came to.
            self.page_size = [page.width().max(0.0) as u32, page.height().max(0.0) as u32];
        } else {
            ui.painter().rect_filled(area, 0.0, ui.visuals().extreme_bg_color);
            let [width, height] = viewport_size(state.viewport, state.size);
            ui.painter().text(
                egui::pos2(area.center().x, page.bottom() + 12.0),
                egui::Align2::CENTER_CENTER,
                format!("{width} × {height} · {:.0}%", zoom * 100.0),
                egui::FontId::proportional(11.0),
                ui.visuals().weak_text_color(),
            );
        }

        let url = normalize_url(&state.address);
        let native = self.native.get_or_insert_with(|| native::NativeBrowser::create(ui.ctx(), frame, &url));
        match native {
            Ok(browser) => {
                if page_visible {
                    browser.place(page, zoom, ui.ctx().pixels_per_point());
                } else {
                    browser.hide();
                }
                let navigating = commands.iter().any(|command| matches!(command, Command::Load(_)));
                for command in commands {
                    browser.run(command);
                }
                // Navigating is asynchronous, so on a frame that asked for one the
                // WebView still reports the page it is leaving. Believing it then
                // would write the outgoing page into this session's saved address,
                // and record it as loaded — so the switch would never be retried
                // and two sessions could end up trading pages for good.
                if !navigating && !address_focused && let Some(url) = browser.url() {
                    state.address = url;
                }
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.as_str());
            }
        }
        // Recorded last, so an address the user typed or a link they followed counts
        // as already loaded rather than as a switch to undo next frame.
        self.loaded = normalize_url(&state.address);
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

/// The page size in CSS pixels, or zeroes for Desktop, which has no size of its
/// own because it takes the panel's.
fn viewport_size(viewport: Viewport, size: [u32; 2]) -> [u32; 2] {
    match viewport {
        Viewport::Desktop => [0, 0],
        Viewport::Fixed => size,
    }
}

/// Where to put the page inside `area`, and how far to zoom it out so a device
/// size that is bigger than the panel still fits while keeping its CSS width.
fn page_rect(area: egui::Rect, viewport: Viewport, size: [u32; 2]) -> (egui::Rect, f32) {
    if viewport == Viewport::Desktop {
        return (area, 1.0);
    }
    const MARGIN: f32 = 12.0;
    const CAPTION: f32 = 24.0;
    let [width, height] = viewport_size(viewport, size).map(|v| v.max(1) as f32);
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
                    self.webview.evaluate_script(&format!(
                        "(window.__viper || window.__barduino) && (window.__viper || window.__barduino).pick({on})"
                    ))
                }
                Command::Comment(on, from) => {
                    if on {
                        let _ = self.webview.focus();
                    }
                    self.webview.evaluate_script(&format!(
                        "(window.__viper || window.__barduino) && (window.__viper || window.__barduino).comment({on}, {from})"
                    ))
                }
                Command::RemovePin(number) => self.webview.evaluate_script(&format!(
                    "(window.__viper || window.__barduino) && (window.__viper || window.__barduino).removePin({number})"
                )),
                Command::ClearPins => self
                    .webview
                    .evaluate_script("(window.__viper || window.__barduino) && (window.__viper || window.__barduino).clearPins()"),
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

        let (mobile, zoom) = page_rect(area, Viewport::Fixed, MOBILE_SIZE);
        assert!(zoom < 1.0, "812 px tall doesn't fit in 600");
        assert!((mobile.width() / mobile.height() - 375.0 / 812.0).abs() < 0.001, "keeps the phone's shape");
        assert!(area.contains_rect(mobile));

        let big = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(2000.0, 2000.0));
        let (tablet, zoom) = page_rect(big, Viewport::Fixed, TABLET_SIZE);
        assert_eq!(zoom, 1.0);
        assert_eq!(tablet.size(), egui::vec2(768.0, 1024.0));
    }

    #[test]
    fn saved_device_presets_become_a_fixed_size() {
        let load = |json: &str| serde_json::from_str::<BrowserState>(json).expect(json);

        // The presets were named before the pixel boxes replaced them.
        let tablet = load(r#"{"address":"localhost:3000","viewport":"Tablet","custom_size":[1280,800]}"#);
        assert_eq!((tablet.viewport, tablet.size), (Viewport::Fixed, TABLET_SIZE));
        assert_eq!(tablet.address, "localhost:3000", "the address survives the move");
        let mobile = load(r#"{"viewport":"Mobile","custom_size":[1280,800]}"#);
        assert_eq!((mobile.viewport, mobile.size), (Viewport::Fixed, MOBILE_SIZE));

        // Custom carried its size in a field of its own.
        let custom = load(r#"{"viewport":"Custom","custom_size":[900,1400]}"#);
        assert_eq!((custom.viewport, custom.size), (Viewport::Fixed, [900, 1400]));

        // Desktop still fills the panel, and saves written since read straight through.
        assert_eq!(load(r#"{"viewport":"Desktop"}"#).viewport, Viewport::Desktop);
        let fixed = load(r#"{"viewport":"Fixed","size":[430,932]}"#);
        assert_eq!((fixed.viewport, fixed.size), (Viewport::Fixed, [430, 932]));
        assert_eq!(load("{}").size, BrowserState::default().size, "an empty save is the default");

        // The app saves in RON, which is stricter than JSON, so the round trip is
        // what actually has to hold.
        let chosen = BrowserState { address: "x".into(), viewport: Viewport::Fixed, size: [430, 932] };
        let written = ron::to_string(&chosen).expect("should save");
        assert_eq!(ron::from_str::<BrowserState>(&written).expect(&written), chosen);
        let old = "(address: \"x\", viewport: Mobile, custom_size: (1280, 800))";
        assert_eq!(ron::from_str::<BrowserState>(old).expect(old).size, MOBILE_SIZE);
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
