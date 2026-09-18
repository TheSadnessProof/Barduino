//! Small icons drawn with egui shapes, so they stay sharp at any size and
//! don't depend on which emoji the font happens to have.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};

#[derive(Clone, Copy)]
pub enum Icon {
    /// A window with a panel on the left, for showing or hiding the session list.
    SidebarLeft,
    /// A window with a panel on the right, for showing or hiding the tools panel.
    SidebarRight,
    Plus,
    More,
    Close,
    /// A crosshair, for picking an element on a web page.
    Pick,
    /// A speech bubble, for leaving a comment on a web page.
    Comment,
    Settings,
    /// A monitor on a stand, for showing a page at the panel's own size.
    Desktop,
    /// A portrait screen with a button, for the tablet page size.
    Tablet,
    /// A narrow portrait screen with a speaker slot, for the phone page size.
    Mobile,
    /// Points at a folded group of sessions.
    ChevronRight,
    /// Points at an open group of sessions.
    ChevronDown,
    /// A search magnifying glass.
    Search,
    /// A git branch icon with a stem and node.
    Branch,
    /// A folder outline.
    Folder,
    /// A wastebasket for deleting.
    Trash,
    /// A pencil for editing or renaming.
    Edit,
}

const SIZE: f32 = 26.0;

/// A square, borderless button with an icon and a tooltip.
pub fn button(ui: &mut egui::Ui, icon: Icon, tooltip: &str) -> egui::Response {
    toggle(ui, icon, tooltip, false)
}

/// A compact icon button, ideal for tab strips and inline close buttons.
pub fn small_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str) -> egui::Response {
    button_sized(ui, icon, 18.0, 3.5, tooltip)
}

/// A button with a specific size and padding.
pub fn button_sized(ui: &mut egui::Ui, icon: Icon, size: f32, padding: f32, tooltip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, false);
        if response.hovered() || response.has_focus() {
            ui.painter().rect_filled(rect, 4.0, visuals.weak_bg_fill);
        }
        let color = if response.hovered() { visuals.fg_stroke.color } else { visuals.text_color() };
        paint(ui.painter(), rect.shrink(padding), icon, color);
    }
    response.on_hover_text(tooltip)
}

/// Like [`button`], but drawn as pressed when `selected` is true.
pub fn toggle(ui: &mut egui::Ui, icon: Icon, tooltip: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(SIZE), Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() || response.has_focus() {
            ui.painter().rect_filled(rect, 5.0, visuals.weak_bg_fill);
        }
        paint(ui.painter(), rect.shrink(6.0), icon, visuals.fg_stroke.color);
    }
    response.on_hover_text(tooltip)
}

pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let c = rect.center();
    let r = rect.width().min(rect.height()) / 2.0;
    match icon {
        Icon::SidebarLeft | Icon::SidebarRight => {
            let frame = Rect::from_center_size(c, vec2(r * 2.0, r * 1.7));
            painter.rect_stroke(frame, 2.0, stroke, egui::StrokeKind::Inside);
            let x = match icon {
                Icon::SidebarLeft => frame.left() + frame.width() * 0.36,
                _ => frame.right() - frame.width() * 0.36,
            };
            painter.line_segment([pos2(x, frame.top()), pos2(x, frame.bottom())], stroke);
        }
        Icon::Plus => {
            painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], stroke);
            painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
        }
        Icon::More => {
            for dx in [-r * 0.75, 0.0, r * 0.75] {
                painter.circle_filled(c + vec2(dx, 0.0), 1.5, color);
            }
        }
        Icon::Close => {
            let close_stroke = if r < 6.0 { Stroke::new(1.2, color) } else { stroke };
            let d = r * 0.72;
            painter.line_segment([c - vec2(d, d), c + vec2(d, d)], close_stroke);
            painter.line_segment([c + vec2(-d, d), c + vec2(d, -d)], close_stroke);
        }
        Icon::Pick => {
            painter.circle_stroke(c, r * 0.6, stroke);
            for (from, to) in [(vec2(0.0, -r), vec2(0.0, -r * 0.3)), (vec2(0.0, r), vec2(0.0, r * 0.3))] {
                painter.line_segment([c + from, c + to], stroke);
            }
            for (from, to) in [(vec2(-r, 0.0), vec2(-r * 0.3, 0.0)), (vec2(r, 0.0), vec2(r * 0.3, 0.0))] {
                painter.line_segment([c + from, c + to], stroke);
            }
        }
        Icon::Comment => {
            let body = Rect::from_min_max(c + vec2(-r, -r * 0.8), c + vec2(r, r * 0.35));
            painter.rect_stroke(body, r * 0.35, stroke, egui::StrokeKind::Middle);
            // The tail, which makes it read as speech rather than a box.
            painter.add(egui::Shape::line(
                vec![
                    c + vec2(-r * 0.45, r * 0.35),
                    c + vec2(-r * 0.55, r * 0.95),
                    c + vec2(0.05 * r, r * 0.35),
                ],
                stroke,
            ));
        }
        // The three page sizes have to be told apart at about 14px, so they lean on
        // shape rather than detail: wide, portrait, and narrow portrait.
        Icon::Desktop => {
            let screen = Rect::from_center_size(c - vec2(0.0, r * 0.18), vec2(r * 1.9, r * 1.3));
            painter.rect_stroke(screen, 1.5, stroke, egui::StrokeKind::Inside);
            let foot = screen.bottom() + r * 0.5;
            painter.line_segment([pos2(c.x, screen.bottom()), pos2(c.x, foot)], stroke);
            painter.line_segment([pos2(c.x - r * 0.5, foot), pos2(c.x + r * 0.5, foot)], stroke);
        }
        Icon::Tablet => {
            let body = Rect::from_center_size(c, vec2(r * 1.4, r * 1.9));
            painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Inside);
            painter.circle_filled(pos2(c.x, body.bottom() - r * 0.24), 1.0, color);
        }
        Icon::Mobile => {
            let body = Rect::from_center_size(c, vec2(r * 0.95, r * 1.9));
            painter.rect_stroke(body, 2.5, stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [pos2(c.x - r * 0.2, body.top() + r * 0.28), pos2(c.x + r * 0.2, body.top() + r * 0.28)],
                stroke,
            );
        }
        Icon::ChevronRight | Icon::ChevronDown => {
            let d = r * 0.5;
            let arm = if matches!(icon, Icon::ChevronRight) {
                [vec2(-d * 0.6, -d), vec2(d * 0.6, 0.0), vec2(-d * 0.6, d)]
            } else {
                [vec2(-d, -d * 0.6), vec2(0.0, d * 0.6), vec2(d, -d * 0.6)]
            };
            painter.add(egui::Shape::line(arm.iter().map(|offset| c + *offset).collect(), stroke));
        }
        Icon::Settings => {
            painter.circle_stroke(c, r * 0.4, stroke);
            for i in 0..8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.0;
                let dir = vec2(angle.cos(), angle.sin());
                let from: Pos2 = c + dir * r * 0.62;
                painter.line_segment([from, c + dir * r], Stroke::new(2.0, color));
            }
            painter.circle_stroke(c, r * 0.68, Stroke::new(1.2, color));
        }
        Icon::Search => {
            let circle_center = c - vec2(r * 0.18, r * 0.18);
            let circle_radius = r * 0.46;
            painter.circle_stroke(circle_center, circle_radius, stroke);
            let handle_start = circle_center + vec2(circle_radius * 0.707, circle_radius * 0.707);
            let handle_end = c + vec2(r * 0.72, r * 0.72);
            painter.line_segment([handle_start, handle_end], Stroke::new(1.8, color));
        }
        Icon::Branch => {
            let trunk_x = c.x - r * 0.32;
            let top_y = c.y - r * 0.52;
            let bot_y = c.y + r * 0.52;
            let right_x = c.x + r * 0.34;
            let node_r = 1.8;

            painter.line_segment([pos2(trunk_x, top_y + node_r), pos2(trunk_x, bot_y - node_r)], stroke);
            painter.add(egui::Shape::line(
                vec![
                    pos2(trunk_x, c.y + r * 0.08),
                    pos2(right_x, c.y - r * 0.12),
                    pos2(right_x, top_y + node_r),
                ],
                stroke,
            ));
            painter.circle_filled(pos2(trunk_x, bot_y), node_r, color);
            painter.circle_filled(pos2(trunk_x, top_y), node_r, color);
            painter.circle_filled(pos2(right_x, top_y), node_r, color);
        }
        Icon::Folder => {
            let left = c.x - r * 0.8;
            let right = c.x + r * 0.8;
            let top = c.y - r * 0.55;
            let bot = c.y + r * 0.6;
            let tab_right = left + r * 0.65;
            let tab_bot = top + r * 0.28;

            painter.add(egui::Shape::line(
                vec![
                    pos2(left, bot),
                    pos2(left, top),
                    pos2(tab_right - r * 0.15, top),
                    pos2(tab_right, tab_bot),
                    pos2(right, tab_bot),
                    pos2(right, bot),
                    pos2(left, bot),
                ],
                stroke,
            ));
        }
        Icon::Trash => {
            let top = c.y - r * 0.65;
            let lid_bot = top + r * 0.22;
            let bot = c.y + r * 0.72;
            let w = r * 0.62;
            painter.line_segment([pos2(c.x - w * 1.15, lid_bot), pos2(c.x + w * 1.15, lid_bot)], stroke);
            painter.line_segment([pos2(c.x - w * 0.35, lid_bot), pos2(c.x - w * 0.35, top)], stroke);
            painter.line_segment([pos2(c.x - w * 0.35, top), pos2(c.x + w * 0.35, top)], stroke);
            painter.line_segment([pos2(c.x + w * 0.35, top), pos2(c.x + w * 0.35, lid_bot)], stroke);
            painter.add(egui::Shape::line(
                vec![
                    pos2(c.x - w * 0.85, lid_bot),
                    pos2(c.x - w * 0.68, bot),
                    pos2(c.x + w * 0.68, bot),
                    pos2(c.x + w * 0.85, lid_bot),
                ],
                stroke,
            ));
        }
        Icon::Edit => {
            let tip = pos2(c.x - r * 0.55, c.y + r * 0.55);
            let top = pos2(c.x + r * 0.45, c.y - r * 0.45);
            let perp = vec2(r * 0.2, -r * 0.2);
            painter.line_segment([tip, top], stroke);
            painter.line_segment([tip + perp, top + perp], stroke);
            painter.line_segment([top, top + perp], stroke);
            painter.line_segment([tip, tip + perp], stroke);
        }
    }
}
