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
    Microphone,
}

const SIZE: f32 = 26.0;

/// A square, borderless button with an icon and a tooltip.
pub fn button(ui: &mut egui::Ui, icon: Icon, tooltip: &str) -> egui::Response {
    toggle(ui, icon, tooltip, false)
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
            let d = r * 0.75;
            painter.line_segment([c - vec2(d, d), c + vec2(d, d)], stroke);
            painter.line_segment([c + vec2(-d, d), c + vec2(d, -d)], stroke);
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
        Icon::Microphone => {
            let body = Rect::from_center_size(c - vec2(0.0, r * 0.3), vec2(r * 0.8, r * 1.3));
            painter.rect_stroke(body, r * 0.4, stroke, egui::StrokeKind::Middle);
            let cup = [pos2(c.x - r * 0.75, c.y - r * 0.1), pos2(c.x - r * 0.7, c.y + r * 0.35), pos2(c.x, c.y + r * 0.6)];
            painter.line(cup.to_vec(), stroke);
            painter.line(cup.iter().map(|p| pos2(2.0 * c.x - p.x, p.y)).collect(), stroke);
            painter.line_segment([pos2(c.x, c.y + r * 0.6), pos2(c.x, c.y + r)], stroke);
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
    }
}
