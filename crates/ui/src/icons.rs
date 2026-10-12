use egui::{vec2, Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};

use crate::theme::TOKENS;
use crate::widgets::Tip;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Logo,
    Move,
    Hand,
    Frame,
    Rectangle,
    Ellipse,
    Line,
    Polygon,
    Star,
    Pen,
    Text,
    Comment,
    ChevronDown,
    ChevronRight,
    Plus,
    Minus,
    Eye,
    EyeOff,
    Lock,
    Search,
    Sparkle,
    Sliders,
    Underline,
    Strike,
    PadH,
    PadV,
    Image,
    Actions,
    Collapse,
    Variables,
    Sidebar,
    Play,
    Help,
    Layers,
    Assets,
    Component,
    Instance,
    Group,
    Vector,
    Boolean,
    More,
    Styles,
    AlignLeft,
    AlignHCenter,
    AlignRight,
    AlignTop,
    AlignVCenter,
    AlignBottom,
    DistributeH,
    DistributeV,
    Rotate,
    FlipH,
    FlipV,
    Angle,
    Opacity,
    Radius,
    LayoutFree,
    LayoutVertical,
    LayoutHorizontal,
    AutoWidth,
    AutoHeight,
    FixedSize,
    TextLeft,
    TextCenter,
    TextRight,
    TextJustify,
    Design,
    Prototype,
    Close,
    Corners,
    ParagraphSpacing,
    LayoutGrid,
    Eyedropper,
    ListNone,
    ListBullet,
    ListNumber,
    Indent,
    Outdent,
}

pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height()) / 16.0;
    let o = rect.center() - vec2(8.0, 8.0) * s;
    let p = |x: f32, y: f32| -> Pos2 { o + vec2(x, y) * s };
    let st = Stroke::new(1.0 * s.max(1.0), color);
    let line = |a: Pos2, b: Pos2| {
        painter.line_segment([a, b], st);
    };
    let poly = |pts: Vec<Pos2>, closed: bool| {
        if closed {
            painter.add(Shape::closed_line(pts, st));
        } else {
            painter.add(Shape::line(pts, st));
        };
    };
    let rect_s = |x0: f32, y0: f32, x1: f32, y1: f32, r: f32| {
        painter.rect_stroke(
            Rect::from_min_max(p(x0, y0), p(x1, y1)),
            r * s,
            st,
            egui::StrokeKind::Middle,
        );
    };
    let fill_rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
        painter.rect_filled(Rect::from_min_max(p(x0, y0), p(x1, y1)), 0.5, color);
    };
    match icon {
        Icon::Logo => {
            painter.rect_filled(
                Rect::from_min_max(p(2.5, 2.5), p(10.5, 10.5)),
                2.5 * s,
                color,
            );
            rect_s(5.5, 5.5, 13.5, 13.5, 2.5);
        }
        Icon::Move => {
            let pts = vec![
                p(4.0, 2.5),
                p(4.0, 13.0),
                p(6.8, 10.3),
                p(8.8, 14.0),
                p(10.4, 13.2),
                p(8.5, 9.6),
                p(12.3, 9.6),
            ];
            painter.add(Shape::convex_polygon(pts.clone(), Color32::TRANSPARENT, st));
            poly(pts, true);
        }
        Icon::Hand => {
            poly(
                vec![
                    p(5.0, 13.5),
                    p(3.0, 9.0),
                    p(4.2, 8.2),
                    p(5.5, 9.5),
                    p(5.5, 3.5),
                    p(6.9, 3.5),
                    p(6.9, 8.0),
                    p(6.9, 2.5),
                    p(8.3, 2.5),
                    p(8.3, 8.0),
                    p(8.3, 3.2),
                    p(9.7, 3.2),
                    p(9.7, 8.3),
                    p(9.7, 4.5),
                    p(11.1, 4.5),
                    p(11.1, 10.5),
                    p(10.0, 13.5),
                ],
                true,
            );
        }
        Icon::Frame => {
            line(p(5.5, 2.0), p(5.5, 14.0));
            line(p(10.5, 2.0), p(10.5, 14.0));
            line(p(2.0, 5.5), p(14.0, 5.5));
            line(p(2.0, 10.5), p(14.0, 10.5));
        }
        Icon::Rectangle => rect_s(3.0, 3.0, 13.0, 13.0, 0.5),
        Icon::Ellipse => {
            painter.circle_stroke(p(8.0, 8.0), 5.5 * s, st);
        }
        Icon::Line => line(p(3.0, 13.0), p(13.0, 3.0)),
        Icon::Polygon => poly(vec![p(8.0, 2.5), p(13.5, 12.5), p(2.5, 12.5)], true),
        Icon::Star => {
            let mut pts = Vec::new();
            for i in 0..10 {
                let a = std::f32::consts::PI * (i as f32) / 5.0 - std::f32::consts::FRAC_PI_2;
                let r = if i % 2 == 0 { 6.0 } else { 2.6 };
                pts.push(p(8.0 + r * a.cos(), 8.5 + r * a.sin()));
            }
            poly(pts, true);
        }
        Icon::Pen => {
            poly(
                vec![
                    p(8.0, 2.5),
                    p(12.0, 8.5),
                    p(9.5, 13.5),
                    p(6.5, 13.5),
                    p(4.0, 8.5),
                ],
                true,
            );
            line(p(8.0, 2.5), p(8.0, 8.0));
            painter.circle_stroke(p(8.0, 9.0), 1.0 * s, st);
        }
        Icon::Text => {
            line(p(3.5, 3.5), p(12.5, 3.5));
            line(p(8.0, 3.5), p(8.0, 13.0));
            line(p(3.5, 3.5), p(3.5, 5.0));
            line(p(12.5, 3.5), p(12.5, 5.0));
            line(p(6.5, 13.0), p(9.5, 13.0));
        }
        Icon::Comment => {
            poly(
                vec![
                    p(3.0, 13.0),
                    p(3.0, 8.0),
                    p(3.5, 5.0),
                    p(5.5, 3.2),
                    p(8.0, 2.8),
                    p(10.5, 3.2),
                    p(12.5, 5.0),
                    p(13.2, 8.0),
                    p(12.5, 11.0),
                    p(10.5, 12.8),
                    p(8.0, 13.2),
                ],
                true,
            );
        }
        Icon::ChevronDown => poly(vec![p(5.0, 6.5), p(8.0, 9.5), p(11.0, 6.5)], false),
        Icon::ChevronRight => poly(vec![p(6.5, 5.0), p(9.5, 8.0), p(6.5, 11.0)], false),
        Icon::Plus => {
            line(p(8.0, 3.5), p(8.0, 12.5));
            line(p(3.5, 8.0), p(12.5, 8.0));
        }
        Icon::Minus => line(p(3.5, 8.0), p(12.5, 8.0)),
        Icon::Close => {
            line(p(4.5, 4.5), p(11.5, 11.5));
            line(p(11.5, 4.5), p(4.5, 11.5));
        }
        Icon::Eye | Icon::EyeOff => {
            poly(
                vec![
                    p(2.0, 8.0),
                    p(4.5, 5.2),
                    p(8.0, 4.2),
                    p(11.5, 5.2),
                    p(14.0, 8.0),
                    p(11.5, 10.8),
                    p(8.0, 11.8),
                    p(4.5, 10.8),
                ],
                true,
            );
            painter.circle_stroke(p(8.0, 8.0), 2.0 * s, st);
            if icon == Icon::EyeOff {
                line(p(3.0, 13.0), p(13.0, 3.0));
            }
        }
        Icon::Lock => {
            rect_s(4.0, 7.5, 12.0, 13.5, 1.0);
            poly(
                vec![
                    p(5.5, 7.5),
                    p(5.5, 5.0),
                    p(6.5, 3.5),
                    p(8.0, 3.0),
                    p(9.5, 3.5),
                    p(10.5, 5.0),
                    p(10.5, 7.5),
                ],
                false,
            );
        }
        Icon::Collapse => {
            for y in [4.5, 8.0, 11.5] {
                line(p(2.5, y), p(9.0, y));
            }
            poly(vec![p(11.0, 6.0), p(13.0, 4.0), p(15.0, 6.0)], false);
            poly(vec![p(11.0, 10.0), p(13.0, 12.0), p(15.0, 10.0)], false);
        }
        Icon::Variables => {
            poly(
                vec![
                    p(8.0, 2.0),
                    p(13.2, 5.0),
                    p(13.2, 11.0),
                    p(8.0, 14.0),
                    p(2.8, 11.0),
                    p(2.8, 5.0),
                ],
                true,
            );
            painter.circle_stroke(p(8.0, 8.0), 2.0 * s, st);
        }
        Icon::Image => {
            rect_s(2.5, 3.5, 13.5, 12.5, 1.0);
            poly(
                vec![
                    p(2.5, 11.0),
                    p(6.0, 7.5),
                    p(9.0, 10.5),
                    p(10.5, 9.0),
                    p(13.5, 12.0),
                ],
                false,
            );
            painter.circle_filled(p(10.5, 6.0), 1.0 * s, color);
        }
        Icon::Actions => {
            painter.circle_stroke(p(5.0, 5.0), 2.2 * s, st);
            painter.circle_stroke(p(11.0, 5.0), 2.2 * s, st);
            painter.circle_stroke(p(5.0, 11.0), 2.2 * s, st);
            line(p(11.0, 8.5), p(11.0, 13.5));
            line(p(8.5, 11.0), p(13.5, 11.0));
        }
        Icon::PadH => {
            line(p(2.5, 3.0), p(2.5, 13.0));
            line(p(13.5, 3.0), p(13.5, 13.0));
            rect_s(5.5, 5.5, 10.5, 10.5, 1.0);
        }
        Icon::PadV => {
            line(p(3.0, 2.5), p(13.0, 2.5));
            line(p(3.0, 13.5), p(13.0, 13.5));
            rect_s(5.5, 5.5, 10.5, 10.5, 1.0);
        }
        Icon::Sliders => {
            line(p(5.0, 2.5), p(5.0, 13.5));
            line(p(11.0, 2.5), p(11.0, 13.5));
            painter.circle_filled(p(5.0, 10.5), 1.8 * s, color);
            painter.circle_filled(p(11.0, 5.5), 1.8 * s, color);
        }
        Icon::Underline => {
            poly(
                vec![
                    p(4.5, 2.5),
                    p(4.5, 8.0),
                    p(5.5, 10.0),
                    p(8.0, 11.0),
                    p(10.5, 10.0),
                    p(11.5, 8.0),
                    p(11.5, 2.5),
                ],
                false,
            );
            line(p(3.5, 13.5), p(12.5, 13.5));
        }
        Icon::Strike => {
            poly(
                vec![
                    p(11.5, 4.0),
                    p(10.0, 2.8),
                    p(8.0, 2.5),
                    p(6.0, 2.8),
                    p(4.8, 4.2),
                    p(5.2, 6.0),
                    p(8.0, 7.2),
                ],
                false,
            );
            poly(
                vec![
                    p(9.8, 9.0),
                    p(11.2, 10.5),
                    p(10.5, 12.5),
                    p(8.0, 13.5),
                    p(5.5, 13.0),
                    p(4.3, 11.8),
                ],
                false,
            );
            line(p(2.5, 8.0), p(13.5, 8.0));
        }
        Icon::Sparkle => {
            poly(
                vec![
                    p(8.0, 2.0),
                    p(9.3, 6.7),
                    p(14.0, 8.0),
                    p(9.3, 9.3),
                    p(8.0, 14.0),
                    p(6.7, 9.3),
                    p(2.0, 8.0),
                    p(6.7, 6.7),
                ],
                true,
            );
        }
        Icon::Search => {
            painter.circle_stroke(p(7.0, 7.0), 4.0 * s, st);
            line(p(10.0, 10.0), p(13.5, 13.5));
        }
        Icon::Sidebar => {
            rect_s(2.5, 3.5, 13.5, 12.5, 1.5);
            line(p(6.5, 3.5), p(6.5, 12.5));
        }
        Icon::Play => {
            poly(vec![p(5.0, 3.5), p(12.5, 8.0), p(5.0, 12.5)], true);
        }
        Icon::Help => {
            poly(
                vec![
                    p(5.8, 6.0),
                    p(6.3, 4.6),
                    p(8.0, 3.8),
                    p(9.7, 4.6),
                    p(10.2, 6.0),
                    p(9.6, 7.3),
                    p(8.0, 8.4),
                    p(8.0, 10.0),
                ],
                false,
            );
            painter.circle_filled(p(8.0, 12.3), 0.8 * s, color);
        }
        Icon::Layers => {
            poly(
                vec![p(8.0, 2.5), p(14.0, 5.8), p(8.0, 9.1), p(2.0, 5.8)],
                true,
            );
            poly(vec![p(2.0, 9.0), p(8.0, 12.3), p(14.0, 9.0)], false);
        }
        Icon::Assets => {
            rect_s(2.5, 2.5, 7.0, 7.0, 1.0);
            rect_s(9.0, 2.5, 13.5, 7.0, 1.0);
            rect_s(2.5, 9.0, 7.0, 13.5, 1.0);
            rect_s(9.0, 9.0, 13.5, 13.5, 1.0);
        }
        Icon::Component => {
            for (cx, cy) in [(8.0, 3.8), (12.2, 8.0), (8.0, 12.2), (3.8, 8.0)] {
                poly(
                    vec![
                        p(cx, cy - 2.2),
                        p(cx + 2.2, cy),
                        p(cx, cy + 2.2),
                        p(cx - 2.2, cy),
                    ],
                    true,
                );
            }
        }
        Icon::Instance => poly(
            vec![p(8.0, 2.5), p(13.5, 8.0), p(8.0, 13.5), p(2.5, 8.0)],
            true,
        ),
        Icon::Group => {
            for (a, b) in [
                ((3.0, 3.0), (5.0, 3.0)),
                ((3.0, 3.0), (3.0, 5.0)),
                ((13.0, 3.0), (11.0, 3.0)),
                ((13.0, 3.0), (13.0, 5.0)),
                ((3.0, 13.0), (5.0, 13.0)),
                ((3.0, 13.0), (3.0, 11.0)),
                ((13.0, 13.0), (11.0, 13.0)),
                ((13.0, 13.0), (13.0, 11.0)),
            ] {
                line(p(a.0, a.1), p(b.0, b.1));
            }
            line(p(7.0, 3.0), p(9.0, 3.0));
            line(p(7.0, 13.0), p(9.0, 13.0));
            line(p(3.0, 7.0), p(3.0, 9.0));
            line(p(13.0, 7.0), p(13.0, 9.0));
        }
        Icon::Vector => {
            poly(
                vec![p(3.0, 12.0), p(6.0, 5.0), p(10.0, 11.0), p(13.0, 4.0)],
                false,
            );
            for (x, y) in [(3.0, 12.0), (13.0, 4.0)] {
                painter.circle_filled(p(x, y), 1.2 * s, color);
            }
        }
        Icon::Boolean => {
            rect_s(2.5, 2.5, 10.0, 10.0, 0.5);
            rect_s(6.0, 6.0, 13.5, 13.5, 0.5);
        }
        Icon::More => {
            for x in [4.0, 8.0, 12.0] {
                painter.circle_filled(p(x, 8.0), 1.0 * s, color);
            }
        }
        Icon::Styles => {
            for (x, y) in [(5.0, 5.0), (11.0, 5.0), (5.0, 11.0), (11.0, 11.0)] {
                painter.circle_stroke(p(x, y), 2.0 * s, st);
            }
        }
        Icon::AlignLeft => {
            line(p(2.5, 2.5), p(2.5, 13.5));
            fill_rect(4.5, 4.5, 12.5, 7.0);
            fill_rect(4.5, 9.0, 9.5, 11.5);
        }
        Icon::AlignHCenter => {
            line(p(8.0, 2.5), p(8.0, 13.5));
            fill_rect(3.5, 4.5, 12.5, 7.0);
            fill_rect(5.5, 9.0, 10.5, 11.5);
        }
        Icon::AlignRight => {
            line(p(13.5, 2.5), p(13.5, 13.5));
            fill_rect(3.5, 4.5, 11.5, 7.0);
            fill_rect(6.5, 9.0, 11.5, 11.5);
        }
        Icon::AlignTop => {
            line(p(2.5, 2.5), p(13.5, 2.5));
            fill_rect(4.5, 4.5, 7.0, 12.5);
            fill_rect(9.0, 4.5, 11.5, 9.5);
        }
        Icon::AlignVCenter => {
            line(p(2.5, 8.0), p(13.5, 8.0));
            fill_rect(4.5, 3.5, 7.0, 12.5);
            fill_rect(9.0, 5.5, 11.5, 10.5);
        }
        Icon::AlignBottom => {
            line(p(2.5, 13.5), p(13.5, 13.5));
            fill_rect(4.5, 3.5, 7.0, 11.5);
            fill_rect(9.0, 6.5, 11.5, 11.5);
        }
        Icon::DistributeH => {
            line(p(2.5, 2.5), p(2.5, 13.5));
            line(p(13.5, 2.5), p(13.5, 13.5));
            fill_rect(6.5, 4.5, 9.5, 11.5);
        }
        Icon::DistributeV => {
            line(p(2.5, 2.5), p(13.5, 2.5));
            line(p(2.5, 13.5), p(13.5, 13.5));
            fill_rect(4.5, 6.5, 11.5, 9.5);
        }
        Icon::Rotate => {
            poly(
                vec![
                    p(12.5, 8.0),
                    p(11.8, 10.6),
                    p(10.0, 12.4),
                    p(8.0, 13.0),
                    p(5.4, 12.3),
                    p(3.6, 10.5),
                    p(3.0, 8.0),
                    p(3.6, 5.5),
                    p(5.4, 3.7),
                    p(8.0, 3.0),
                    p(10.5, 3.6),
                ],
                false,
            );
            poly(vec![p(9.8, 1.6), p(10.8, 3.6), p(8.8, 4.8)], false);
        }
        Icon::FlipH => {
            line(p(8.0, 2.0), p(8.0, 14.0));
            poly(vec![p(6.0, 4.0), p(2.5, 12.0), p(6.0, 12.0)], true);
            poly(vec![p(10.0, 4.0), p(13.5, 12.0), p(10.0, 12.0)], true);
        }
        Icon::FlipV => {
            line(p(2.0, 8.0), p(14.0, 8.0));
            poly(vec![p(4.0, 6.0), p(12.0, 2.5), p(12.0, 6.0)], true);
            poly(vec![p(4.0, 10.0), p(12.0, 13.5), p(12.0, 10.0)], true);
        }
        Icon::Angle => {
            poly(vec![p(3.0, 3.0), p(3.0, 13.0), p(13.0, 13.0)], false);
            poly(
                vec![p(3.0, 8.0), p(5.5, 8.6), p(7.5, 10.5), p(8.0, 13.0)],
                false,
            );
        }
        Icon::Opacity => {
            rect_s(3.0, 3.0, 13.0, 13.0, 1.0);
            fill_rect(3.0, 3.0, 8.0, 8.0);
            fill_rect(8.0, 8.0, 13.0, 13.0);
        }
        Icon::Radius => {
            poly(
                vec![
                    p(3.0, 13.0),
                    p(3.0, 8.0),
                    p(3.6, 5.5),
                    p(5.5, 3.6),
                    p(8.0, 3.0),
                    p(13.0, 3.0),
                ],
                false,
            );
        }
        Icon::Corners => {
            for (x, y, dx, dy) in [
                (3.0, 3.0, 1.0, 1.0),
                (13.0, 3.0, -1.0, 1.0),
                (13.0, 13.0, -1.0, -1.0),
                (3.0, 13.0, 1.0, -1.0),
            ] {
                poly(vec![p(x, y + 4.0 * dy), p(x, y), p(x + 4.0 * dx, y)], false);
            }
        }
        Icon::Eyedropper => {
            poly(
                vec![
                    p(9.5, 4.0),
                    p(12.0, 6.5),
                    p(4.5, 14.0),
                    p(2.5, 14.0),
                    p(2.5, 12.0),
                    p(10.0, 4.5),
                ],
                true,
            );
            line(p(10.5, 2.0), p(14.5, 6.0));
            line(p(12.5, 1.5), p(14.5, 3.5));
        }
        Icon::LayoutGrid => {
            fill_rect(3.0, 3.0, 7.0, 7.0);
            fill_rect(9.0, 3.0, 13.0, 7.0);
            fill_rect(3.0, 9.0, 7.0, 13.0);
            fill_rect(9.0, 9.0, 13.0, 13.0);
        }
        Icon::ParagraphSpacing => {
            line(p(5.0, 3.0), p(13.5, 3.0));
            line(p(5.0, 5.5), p(13.5, 5.5));
            line(p(5.0, 10.5), p(13.5, 10.5));
            line(p(5.0, 13.0), p(13.5, 13.0));
            line(p(2.5, 6.5), p(2.5, 9.5));
        }
        Icon::ListNone => {
            for y in [4.0, 8.0, 12.0] {
                line(p(2.5, y), p(13.5, y));
            }
        }
        Icon::ListBullet => {
            for y in [4.0, 8.0, 12.0] {
                fill_rect(2.0, y - 1.0, 4.0, y + 1.0);
                line(p(6.5, y), p(13.5, y));
            }
        }
        Icon::ListNumber => {
            line(p(3.0, 2.5), p(3.0, 5.5));
            poly(
                vec![
                    p(2.0, 6.5),
                    p(4.0, 6.5),
                    p(4.0, 8.0),
                    p(2.0, 8.0),
                    p(2.0, 9.5),
                    p(4.0, 9.5),
                ],
                false,
            );
            poly(
                vec![p(2.0, 10.5), p(4.0, 10.5), p(4.0, 13.5), p(2.0, 13.5)],
                false,
            );
            line(p(2.5, 12.0), p(4.0, 12.0));
            for y in [4.0, 8.0, 12.0] {
                line(p(6.5, y), p(13.5, y));
            }
        }
        Icon::Indent | Icon::Outdent => {
            line(p(2.5, 3.0), p(13.5, 3.0));
            line(p(7.0, 6.5), p(13.5, 6.5));
            line(p(7.0, 9.5), p(13.5, 9.5));
            line(p(2.5, 13.0), p(13.5, 13.0));
            let (tip, back) = if icon == Icon::Indent {
                (5.0, 2.5)
            } else {
                (2.5, 5.0)
            };
            poly(vec![p(back, 6.0), p(tip, 8.0), p(back, 10.0)], false);
        }
        Icon::LayoutFree => {
            rect_s(2.5, 2.5, 13.5, 13.5, 1.5);
            fill_rect(5.0, 5.0, 8.0, 8.0);
        }
        Icon::LayoutVertical => {
            fill_rect(4.0, 3.0, 12.0, 6.5);
            fill_rect(4.0, 9.5, 12.0, 13.0);
        }
        Icon::LayoutHorizontal => {
            fill_rect(3.0, 4.0, 6.5, 12.0);
            fill_rect(9.5, 4.0, 13.0, 12.0);
        }
        Icon::AutoWidth => {
            line(p(2.5, 8.0), p(13.5, 8.0));
            poly(vec![p(4.5, 6.0), p(2.5, 8.0), p(4.5, 10.0)], false);
            poly(vec![p(11.5, 6.0), p(13.5, 8.0), p(11.5, 10.0)], false);
        }
        Icon::AutoHeight => {
            line(p(8.0, 2.5), p(8.0, 13.5));
            poly(vec![p(6.0, 4.5), p(8.0, 2.5), p(10.0, 4.5)], false);
            poly(vec![p(6.0, 11.5), p(8.0, 13.5), p(10.0, 11.5)], false);
        }
        Icon::FixedSize => rect_s(3.0, 3.0, 13.0, 13.0, 1.0),
        Icon::TextLeft | Icon::TextCenter | Icon::TextRight | Icon::TextJustify => {
            let rows = [(3.0, 13.0), (3.0, 10.0), (3.0, 13.0), (3.0, 9.0)];
            for (i, (x0, x1)) in rows.iter().enumerate() {
                let y = 4.0 + i as f32 * 2.8;
                let w = x1 - x0;
                let (a, b) = match icon {
                    Icon::TextLeft => (*x0, *x1),
                    Icon::TextRight => (16.0 - x1, 16.0 - x0),
                    Icon::TextCenter => (8.0 - w / 2.0, 8.0 + w / 2.0),
                    _ => (3.0, 13.0),
                };
                line(p(a, y), p(b, y));
            }
        }
        Icon::Design => {
            poly(
                vec![
                    p(3.0, 13.0),
                    p(4.0, 9.5),
                    p(11.0, 2.5),
                    p(13.5, 5.0),
                    p(6.5, 12.0),
                ],
                true,
            );
        }
        Icon::Prototype => {
            painter.circle_stroke(p(4.5, 11.5), 1.8 * s, st);
            poly(vec![p(6.0, 10.0), p(9.0, 5.0), p(12.5, 5.0)], false);
            poly(vec![p(10.8, 3.3), p(12.5, 5.0), p(10.8, 6.7)], false);
        }
    }
}

pub fn button(ui: &mut Ui, icon: Icon, selected: bool, tooltip: &str) -> Response {
    sized_button(ui, icon, selected, tooltip, Vec2::splat(24.0))
}

pub fn sized_button(
    ui: &mut Ui,
    icon: Icon,
    selected: bool,
    tooltip: &str,
    size: Vec2,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    let color = if selected {
        painter.rect_filled(rect, 5.0, TOKENS.row_selected);
        TOKENS.accent
    } else {
        if resp.hovered() {
            painter.rect_filled(rect, 5.0, TOKENS.row_hover);
        }
        TOKENS.text
    };
    paint(
        painter,
        Rect::from_center_size(rect.center(), Vec2::splat(16.0)),
        icon,
        color,
    );
    if tooltip.is_empty() {
        resp
    } else {
        resp.tip(tooltip)
    }
}

pub fn glyph(ui: &mut Ui, icon: Icon, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
    paint(ui.painter(), rect, icon, color);
    resp
}

pub fn tile(ui: &mut Ui, icon: Icon, active: bool, tooltip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, 8.0, TOKENS.tile_active);
    } else if resp.hovered() {
        painter.rect_filled(rect, 8.0, TOKENS.row_hover);
    }
    let color = if active { TOKENS.accent } else { TOKENS.text };
    paint(
        painter,
        Rect::from_center_size(rect.center(), Vec2::splat(16.0)),
        icon,
        color,
    );
    resp.tip(tooltip)
}
