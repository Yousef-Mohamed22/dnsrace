//! A small hand-drawn line-icon set for navigation and actions.
//!
//! Icons are painted with egui shapes instead of a font, so they inherit the text colour and
//! stay crisp at every size without another dependency.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};

/// Every icon used by the desktop and mobile shells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Gauge,
    Server,
    History,
    Settings,
    Star,
    StarFilled,
    Refresh,
    Copy,
    Play,
    Stop,
    ChevronRight,
    X,
    Search,
    Download,
    Share,
    Check,
    Alert,
    Info,
    External,
}

impl Icon {
    pub const ALL: [Icon; 19] = [
        Icon::Gauge,
        Icon::Server,
        Icon::History,
        Icon::Settings,
        Icon::Star,
        Icon::StarFilled,
        Icon::Refresh,
        Icon::Copy,
        Icon::Play,
        Icon::Stop,
        Icon::ChevronRight,
        Icon::X,
        Icon::Search,
        Icon::Download,
        Icon::Share,
        Icon::Check,
        Icon::Alert,
        Icon::Info,
        Icon::External,
    ];
}

/// Paint an icon at `size` logical points. The response is the allocated square.
pub fn paint(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) -> egui::Response {
    paint_in(ui, icon, size, color, Sense::hover())
}

/// Paint an icon and allocate it with the caller's sense (so it can be clickable).
pub fn paint_in(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    color: Color32,
    sense: Sense,
) -> egui::Response {
    let size = size.max(16.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), sense);
    paint_at(&ui.painter().with_clip_rect(rect), icon, rect, color);
    response
}

/// Paint an icon into an existing rectangle. Shells use this to compose one large hit target
/// with a painted icon while keeping a single accessible label on the surrounding widget.
pub fn paint_at(painter: &egui::Painter, icon: Icon, rect: Rect, color: Color32) {
    let stroke_width = (rect.width() * 0.0875).clamp(1.25, 2.0);
    draw(painter, icon, rect, color, Stroke::new(stroke_width, color));
}

/// The dnsrace app mark: a deep-ink rounded tile, three ascending bars and a mint pulse dot.
pub fn paint_logo(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(
        rect,
        rect.width() * 0.26,
        Color32::from_rgb(0x0E, 0x1A, 0x2C),
    );

    let inner = rect.shrink(rect.width() * 0.235);
    let bar_width = inner.width() * 0.18;
    let gap = inner.width() * 0.12;
    let colors = [
        Color32::from_rgb(0x38, 0xBD, 0xF8),
        Color32::from_rgb(0x2D, 0xCC, 0xD2),
        Color32::from_rgb(0x34, 0xD3, 0x99),
    ];
    let heights = [0.38, 0.58, 0.82];
    let mut x = inner.left();
    for (height, color) in heights.into_iter().zip(colors) {
        let bar = Rect::from_min_max(
            Pos2::new(x, inner.bottom() - inner.height() * height),
            Pos2::new(x + bar_width, inner.bottom()),
        );
        painter.rect_filled(bar, 1.5, color);
        x += bar_width + gap;
    }
    painter.circle_filled(
        Pos2::new(
            rect.right() - rect.width() * 0.235,
            rect.top() + rect.width() * 0.235,
        ),
        rect.width() * 0.07,
        Color32::from_rgb(0x34, 0xD3, 0x99),
    );
}

fn draw(painter: &egui::Painter, icon: Icon, rect: Rect, color: Color32, stroke: Stroke) {
    let point = |x: f32, y: f32| rect.min + Vec2::new(rect.width() * x, rect.height() * y);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([point(a.0, a.1), point(b.0, b.1)], stroke);
    };
    match icon {
        Icon::Gauge => {
            let center = point(0.5, 0.64);
            let radius = rect.width() * 0.34;
            painter.add(Shape::line(arc(center, radius, 180.0, 360.0, 24), stroke));
            painter.line_segment(
                [center, point(0.70, 0.40)],
                Stroke::new(stroke.width * 1.4, color),
            );
            painter.circle_filled(center, rect.width() * 0.055, color);
            line((0.5, 0.64), (0.5, 0.68));
        }
        Icon::Server => {
            let top = Rect::from_min_max(point(0.20, 0.24), point(0.80, 0.46));
            let bottom = Rect::from_min_max(point(0.20, 0.54), point(0.80, 0.76));
            painter.rect_stroke(top, 3.0, stroke, StrokeKind::Inside);
            painter.rect_stroke(bottom, 3.0, stroke, StrokeKind::Inside);
            painter.circle_filled(point(0.31, 0.35), rect.width() * 0.035, color);
            painter.circle_filled(point(0.31, 0.65), rect.width() * 0.035, color);
            line((0.44, 0.35), (0.68, 0.35));
            line((0.44, 0.65), (0.68, 0.65));
        }
        Icon::History => {
            painter.circle_stroke(point(0.5, 0.5), rect.width() * 0.33, stroke);
            line((0.5, 0.5), (0.5, 0.29));
            line((0.5, 0.5), (0.66, 0.60));
        }
        Icon::Settings => {
            let center = point(0.5, 0.5);
            let inner = rect.width() * 0.15;
            painter.circle_stroke(center, inner, stroke);
            for index in 0..8 {
                let angle = index as f32 * std::f32::consts::TAU / 8.0;
                let direction = Vec2::angled(angle);
                painter.line_segment(
                    [
                        center + direction * (inner + rect.width() * 0.07),
                        center + direction * (inner + rect.width() * 0.18),
                    ],
                    stroke,
                );
            }
        }
        Icon::Star | Icon::StarFilled => {
            let points = star_points(rect);
            if icon == Icon::StarFilled {
                // A star is concave, and epaint fills only convex paths. The shape is
                // star-shaped around its centre, so a centre fan covers it exactly.
                let center = point(0.5, 0.5);
                for index in 0..points.len() {
                    let next = (index + 1) % points.len();
                    painter.add(Shape::convex_polygon(
                        vec![center, points[index], points[next]],
                        color,
                        Stroke::NONE,
                    ));
                }
            } else {
                painter.add(Shape::closed_line(points, stroke));
            }
        }
        Icon::Refresh => {
            let center = point(0.5, 0.5);
            let radius = rect.width() * 0.29;
            painter.add(Shape::line(arc(center, radius, 30.0, 320.0, 22), stroke));
            let tip = point(0.66, 0.24);
            painter.line_segment([tip, point(0.76, 0.27)], stroke);
            painter.line_segment([tip, point(0.70, 0.37)], stroke);
        }
        Icon::Copy => {
            let back = Rect::from_min_max(point(0.26, 0.24), point(0.62, 0.62));
            let front = Rect::from_min_max(point(0.38, 0.38), point(0.74, 0.76));
            painter.rect_stroke(back, 3.0, stroke, StrokeKind::Inside);
            painter.rect_stroke(front, 3.0, stroke, StrokeKind::Inside);
        }
        Icon::Play => {
            painter.add(Shape::convex_polygon(
                vec![point(0.34, 0.24), point(0.34, 0.76), point(0.76, 0.5)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Stop => {
            painter.rect_filled(
                Rect::from_min_max(point(0.32, 0.32), point(0.68, 0.68)),
                2.0,
                color,
            );
        }
        Icon::ChevronRight => {
            line((0.40, 0.26), (0.62, 0.5));
            line((0.62, 0.5), (0.40, 0.74));
        }
        Icon::X => {
            line((0.3, 0.3), (0.7, 0.7));
            line((0.7, 0.3), (0.3, 0.7));
        }
        Icon::Search => {
            painter.circle_stroke(point(0.44, 0.44), rect.width() * 0.22, stroke);
            line((0.60, 0.60), (0.76, 0.76));
        }
        Icon::Download => {
            line((0.5, 0.20), (0.5, 0.60));
            line((0.34, 0.46), (0.5, 0.62));
            line((0.66, 0.46), (0.5, 0.62));
            line((0.24, 0.68), (0.24, 0.78));
            line((0.24, 0.78), (0.76, 0.78));
            line((0.76, 0.78), (0.76, 0.68));
        }
        Icon::Share => {
            painter.circle_stroke(point(0.27, 0.5), rect.width() * 0.075, stroke);
            painter.circle_stroke(point(0.72, 0.27), rect.width() * 0.075, stroke);
            painter.circle_stroke(point(0.72, 0.73), rect.width() * 0.075, stroke);
            line((0.33, 0.46), (0.66, 0.31));
            line((0.33, 0.54), (0.66, 0.69));
        }
        Icon::Check => {
            line((0.26, 0.52), (0.44, 0.7));
            line((0.44, 0.7), (0.74, 0.32));
        }
        Icon::Alert => {
            painter.add(Shape::closed_line(
                vec![point(0.5, 0.22), point(0.82, 0.78), point(0.18, 0.78)],
                stroke,
            ));
            line((0.5, 0.4), (0.5, 0.58));
            painter.circle_filled(point(0.5, 0.68), rect.width() * 0.03, color);
        }
        Icon::Info => {
            painter.circle_stroke(point(0.5, 0.5), rect.width() * 0.32, stroke);
            line((0.5, 0.46), (0.5, 0.68));
            painter.circle_filled(point(0.5, 0.34), rect.width() * 0.03, color);
        }
        Icon::External => {
            let box_rect = Rect::from_min_max(point(0.24, 0.32), point(0.62, 0.76));
            painter.rect_stroke(box_rect, 2.0, stroke, StrokeKind::Inside);
            line((0.44, 0.56), (0.76, 0.24));
            line((0.60, 0.24), (0.76, 0.24));
            line((0.76, 0.24), (0.76, 0.40));
        }
    }
}

/// The ten alternating outer/inner vertices of a five-point star.
fn star_points(rect: Rect) -> Vec<Pos2> {
    (0..10)
        .map(|index| {
            let radius = if index % 2 == 0 {
                rect.width() * 0.36
            } else {
                rect.width() * 0.15
            };
            let angle = -std::f32::consts::FRAC_PI_2 + index as f32 * std::f32::consts::PI / 5.0;
            rect.center() + Vec2::new(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

/// Points along a circular arc. Angles are degrees, clockwise in screen coordinates.
fn arc(center: Pos2, radius: f32, start_deg: f32, end_deg: f32, steps: usize) -> Vec<Pos2> {
    let steps = steps.max(2);
    (0..=steps)
        .map(|index| {
            let angle =
                (start_deg + (end_deg - start_deg) * index as f32 / steps as f32).to_radians();
            center + Vec2::new(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle_area(a: Pos2, b: Pos2, c: Pos2) -> f32 {
        ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)).abs() * 0.5
    }

    fn winding(center: Pos2, a: Pos2, b: Pos2) -> f32 {
        (a.x - center.x) * (b.y - center.y) - (b.x - center.x) * (a.y - center.y)
    }

    #[test]
    fn filled_star_fan_covers_the_outline_without_folding() {
        let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::splat(40.0));
        let points = star_points(rect);
        assert_eq!(points.len(), 10);

        let center = rect.center();
        let mut fan_area = 0.0;
        for index in 0..points.len() {
            let a = points[index];
            let b = points[(index + 1) % points.len()];
            assert!(
                winding(center, a, b) > 0.0,
                "star fan triangle {index} folds over the centre"
            );
            fan_area += triangle_area(center, a, b);
        }

        let polygon_area = (0..points.len())
            .map(|index| {
                let a = points[index];
                let b = points[(index + 1) % points.len()];
                a.x * b.y - b.x * a.y
            })
            .sum::<f32>()
            .abs()
            * 0.5;
        assert!(
            (fan_area - polygon_area).abs() < 0.01,
            "the centre fan must cover the star exactly: {fan_area} vs {polygon_area}"
        );
    }
}
