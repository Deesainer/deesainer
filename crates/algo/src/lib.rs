use deesainer_doc::{AutoLayout, CounterAlign, LayoutDirection, PrimaryAlign, SizeLimits, Sizing};
use deesainer_geom::Rect;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Child {
    pub width: f64,
    pub height: f64,
    pub horizontal: Sizing,
    pub vertical: Sizing,
    pub hug_width: f64,
    pub hug_height: f64,
    pub skip: bool,
    pub limits: SizeLimits,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    pub width: f64,
    pub height: f64,
    pub rects: Vec<Option<Rect>>,
}

pub type Reflow<'a> = &'a dyn Fn(usize, f64) -> f64;

struct Axis {
    horizontal: bool,
}

impl Axis {
    fn main(&self, w: f64, h: f64) -> f64 {
        if self.horizontal {
            w
        } else {
            h
        }
    }

    fn cross(&self, w: f64, h: f64) -> f64 {
        if self.horizontal {
            h
        } else {
            w
        }
    }
}

fn finite(v: f64) -> f64 {
    if v.is_finite() {
        v.max(0.0)
    } else {
        0.0
    }
}

pub fn solve(
    layout: &AutoLayout,
    width: f64,
    height: f64,
    children: &[Child],
    reflow: Reflow<'_>,
) -> Solution {
    solve_limited(
        layout,
        width,
        height,
        children,
        SizeLimits::default(),
        reflow,
    )
}

pub fn solve_limited(
    layout: &AutoLayout,
    width: f64,
    height: f64,
    children: &[Child],
    frame_limits: SizeLimits,
    reflow: Reflow<'_>,
) -> Solution {
    if layout.direction == LayoutDirection::Grid {
        return solve_grid(layout, width, height, children, frame_limits);
    }
    let axis = Axis {
        horizontal: layout.direction == LayoutDirection::Horizontal,
    };
    let pad = layout.padding;
    let (pad_main_start, pad_main_end, pad_cross_start, pad_cross_end) = if axis.horizontal {
        (pad.left, pad.right, pad.top, pad.bottom)
    } else {
        (pad.top, pad.bottom, pad.left, pad.right)
    };
    let gap = finite(layout.gap);
    let frame_main_fixed = axis.main(width, height);
    let frame_cross_fixed = axis.cross(width, height);
    let hug_main = layout.primary_sizing == Sizing::Hug;
    let hug_cross = layout.counter_sizing == Sizing::Hug;

    let flow: Vec<usize> = (0..children.len()).filter(|i| !children[*i].skip).collect();
    let clamp_main = |c: &Child, v: f64| {
        if axis.horizontal {
            c.limits.width(v)
        } else {
            c.limits.height(v)
        }
    };
    let clamp_cross = |c: &Child, v: f64| {
        if axis.horizontal {
            c.limits.height(v)
        } else {
            c.limits.width(v)
        }
    };
    let (frame_clamp_main, frame_clamp_cross) = (
        |v: f64| {
            if axis.horizontal {
                frame_limits.width(v)
            } else {
                frame_limits.height(v)
            }
        },
        |v: f64| {
            if axis.horizontal {
                frame_limits.height(v)
            } else {
                frame_limits.width(v)
            }
        },
    );
    let sizing_main = |c: &Child| {
        if axis.horizontal {
            c.horizontal
        } else {
            c.vertical
        }
    };
    let sizing_cross = |c: &Child| {
        if axis.horizontal {
            c.vertical
        } else {
            c.horizontal
        }
    };

    let base_main = |c: &Child| {
        finite(clamp_main(
            c,
            match sizing_main(c) {
                Sizing::Fixed => axis.main(c.width, c.height),
                Sizing::Hug | Sizing::Fill => axis.main(c.hug_width, c.hug_height),
            },
        ))
    };
    let base_cross = |c: &Child| {
        finite(clamp_cross(
            c,
            match sizing_cross(c) {
                Sizing::Fixed => axis.cross(c.width, c.height),
                Sizing::Hug | Sizing::Fill => axis.cross(c.hug_width, c.hug_height),
            },
        ))
    };

    let inner_main_fixed = (frame_main_fixed - pad_main_start - pad_main_end).max(0.0);
    let mut lines: Vec<Vec<usize>> = Vec::new();
    if layout.wrap && axis.horizontal && !hug_main {
        let mut current: Vec<usize> = Vec::new();
        let mut used = 0.0;
        for &i in &flow {
            let m = base_main(&children[i]);
            let needed = if current.is_empty() {
                m
            } else {
                used + gap + m
            };
            if !current.is_empty() && needed > inner_main_fixed + 1e-9 {
                lines.push(std::mem::take(&mut current));
                used = m;
            } else {
                used = needed;
            }
            current.push(i);
        }
        if !current.is_empty() {
            lines.push(current);
        }
    } else {
        lines.push(flow.clone());
    }

    let line_main_content = |line: &[usize]| -> f64 {
        let sum: f64 = line.iter().map(|i| base_main(&children[*i])).sum();
        sum + gap * line.len().saturating_sub(1) as f64
    };
    let frame_main = if hug_main {
        frame_clamp_main(
            pad_main_start
                + pad_main_end
                + lines
                    .iter()
                    .map(|l| line_main_content(l))
                    .fold(0.0, f64::max),
        )
    } else {
        frame_main_fixed
    };
    let inner_main = (frame_main - pad_main_start - pad_main_end).max(0.0);

    let mut main_size = vec![0.0; children.len()];
    let mut cross_size = vec![0.0; children.len()];
    for line in &lines {
        let fills: Vec<usize> = line
            .iter()
            .copied()
            .filter(|i| sizing_main(&children[*i]) == Sizing::Fill && !hug_main)
            .collect();
        let fixed_sum: f64 = line
            .iter()
            .filter(|i| !fills.contains(i))
            .map(|i| base_main(&children[*i]))
            .sum();
        let gaps = gap * line.len().saturating_sub(1) as f64;
        let free = (inner_main - fixed_sum - gaps).max(0.0);
        let mut open = fills.clone();
        let mut left = free;
        while !open.is_empty() {
            let share = (left / open.len() as f64).max(0.0);
            let frozen: Vec<usize> = open
                .iter()
                .copied()
                .filter(|i| (clamp_main(&children[*i], share) - share).abs() > 1e-9)
                .collect();
            if frozen.is_empty() {
                for &i in &open {
                    main_size[i] = share;
                }
                break;
            }
            for i in frozen {
                let v = finite(clamp_main(&children[i], share));
                main_size[i] = v;
                left -= v;
                open.retain(|o| *o != i);
            }
        }
        for &i in line {
            if !fills.contains(&i) {
                main_size[i] = base_main(&children[i]);
            }
            cross_size[i] = base_cross(&children[i]);
        }
    }

    for &i in &flow {
        let c = &children[i];
        if axis.horizontal && c.horizontal == Sizing::Fill && c.vertical != Sizing::Fixed {
            cross_size[i] = finite(clamp_cross(c, reflow(i, main_size[i])));
        }
    }

    let counter_gap = finite(layout.counter_gap);
    let line_cross: Vec<f64> = lines
        .iter()
        .map(|l| {
            l.iter()
                .filter(|i| sizing_cross(&children[**i]) != Sizing::Fill || hug_cross)
                .map(|i| cross_size[*i])
                .fold(0.0, f64::max)
        })
        .collect();
    let content_cross: f64 =
        line_cross.iter().sum::<f64>() + counter_gap * lines.len().saturating_sub(1) as f64;
    let frame_cross = if hug_cross {
        frame_clamp_cross(pad_cross_start + pad_cross_end + content_cross)
    } else {
        frame_cross_fixed
    };
    let inner_cross = (frame_cross - pad_cross_start - pad_cross_end).max(0.0);

    for (li, line) in lines.iter().enumerate() {
        let extent = if lines.len() == 1 {
            inner_cross
        } else {
            line_cross[li]
        };
        for &i in line {
            let c = &children[i];
            if sizing_cross(c) == Sizing::Fill && !hug_cross {
                cross_size[i] = finite(clamp_cross(c, extent));
                if !axis.horizontal && c.horizontal == Sizing::Fill && c.vertical != Sizing::Fixed {
                    main_size[i] = finite(clamp_main(c, reflow(i, cross_size[i])));
                }
            }
        }
    }

    let mut rects: Vec<Option<Rect>> = vec![None; children.len()];
    let mut cross_cursor = pad_cross_start;
    let main_total_hug_check = lines.iter().map(|l| {
        let sum: f64 = l.iter().map(|i| main_size[*i]).sum();
        sum + gap * l.len().saturating_sub(1) as f64
    });
    let line_totals: Vec<f64> = main_total_hug_check.collect();
    let frame_main = if hug_main && !axis.horizontal {
        frame_clamp_main(
            pad_main_start + pad_main_end + line_totals.iter().copied().fold(0.0, f64::max),
        )
    } else {
        frame_main
    };
    let inner_main = (frame_main - pad_main_start - pad_main_end).max(0.0);
    for (li, line) in lines.iter().enumerate() {
        let total = line_totals[li];
        let n = line.len();
        let (mut cursor, step_gap) = match layout.primary_align {
            PrimaryAlign::Start => (pad_main_start, gap),
            PrimaryAlign::Center => (pad_main_start + (inner_main - total) / 2.0, gap),
            PrimaryAlign::End => (pad_main_start + inner_main - total, gap),
            PrimaryAlign::SpaceBetween => {
                if n > 1 {
                    let sum: f64 = line.iter().map(|i| main_size[*i]).sum();
                    (
                        pad_main_start,
                        ((inner_main - sum) / (n - 1) as f64).max(0.0),
                    )
                } else {
                    (pad_main_start, gap)
                }
            }
        };
        let extent = if lines.len() == 1 {
            inner_cross
        } else {
            line_cross[li]
        };
        for &i in line {
            let m = main_size[i];
            let c = cross_size[i];
            let cross_off = match layout.counter_align {
                CounterAlign::Start => 0.0,
                CounterAlign::Center => (extent - c) / 2.0,
                CounterAlign::End => extent - c,
            };
            let (x, y, w, h) = if axis.horizontal {
                (cursor, cross_cursor + cross_off, m, c)
            } else {
                (cross_cursor + cross_off, cursor, c, m)
            };
            rects[i] = Some(Rect::new(x, y, w, h));
            cursor += m + step_gap;
        }
        cross_cursor += line_cross[li] + counter_gap;
    }
    let (w, h) = if axis.horizontal {
        (frame_main, frame_cross)
    } else {
        (frame_cross, frame_main)
    };
    Solution {
        width: finite(w),
        height: finite(h),
        rects,
    }
}

fn solve_grid(
    layout: &AutoLayout,
    width: f64,
    height: f64,
    children: &[Child],
    frame_limits: SizeLimits,
) -> Solution {
    let pad = layout.padding;
    let (gap, row_gap) = (finite(layout.gap), finite(layout.counter_gap));
    let flow: Vec<usize> = (0..children.len()).filter(|i| !children[*i].skip).collect();
    let cols = layout.columns();
    let needed = flow.len().div_ceil(cols).max(1);
    let rows = needed.max(layout.grid_rows as usize);
    let hug_w = layout.primary_sizing == Sizing::Hug;
    let hug_h = layout.counter_sizing == Sizing::Hug;
    let base = |c: &Child, horizontal: bool| {
        let (fixed, hug, sizing) = if horizontal {
            (c.width, c.hug_width, c.horizontal)
        } else {
            (c.height, c.hug_height, c.vertical)
        };
        let v = if sizing == Sizing::Fixed { fixed } else { hug };
        finite(if horizontal {
            c.limits.width(v)
        } else {
            c.limits.height(v)
        })
    };
    let col_w = if hug_w {
        flow.iter()
            .map(|i| base(&children[*i], true))
            .fold(0.0, f64::max)
    } else {
        ((width - pad.left - pad.right - gap * (cols - 1) as f64) / cols as f64).max(0.0)
    };
    let frame_w = if hug_w {
        frame_limits.width(pad.left + pad.right + col_w * cols as f64 + gap * (cols - 1) as f64)
    } else {
        width
    };
    let shared_rows = !hug_h && layout.grid_rows > 0;
    let row_h: Vec<f64> = (0..rows)
        .map(|r| {
            if shared_rows {
                ((height - pad.top - pad.bottom - row_gap * (rows - 1) as f64) / rows as f64)
                    .max(0.0)
            } else {
                flow.iter()
                    .skip(r * cols)
                    .take(cols)
                    .map(|i| base(&children[*i], false))
                    .fold(0.0, f64::max)
            }
        })
        .collect();
    let content_h: f64 = row_h.iter().sum::<f64>() + row_gap * (rows - 1) as f64;
    let frame_h = if hug_h {
        frame_limits.height(pad.top + pad.bottom + content_h)
    } else {
        height
    };
    let mut rects = vec![None; children.len()];
    let mut y = pad.top;
    for (r, h) in row_h.iter().enumerate() {
        for (c, i) in flow.iter().skip(r * cols).take(cols).enumerate() {
            let ch = &children[*i];
            let w = if ch.horizontal == Sizing::Fill {
                ch.limits.width(col_w)
            } else {
                base(ch, true)
            };
            let hh = if ch.vertical == Sizing::Fill {
                ch.limits.height(*h)
            } else {
                base(ch, false)
            };
            let x0 = pad.left + c as f64 * (col_w + gap);
            let dx = match layout.primary_align {
                PrimaryAlign::Center => (col_w - w) / 2.0,
                PrimaryAlign::End => col_w - w,
                PrimaryAlign::Start | PrimaryAlign::SpaceBetween => 0.0,
            };
            let dy = match layout.counter_align {
                CounterAlign::Center => (h - hh) / 2.0,
                CounterAlign::End => h - hh,
                CounterAlign::Start => 0.0,
            };
            rects[*i] = Some(Rect::new(x0 + dx, y + dy, finite(w), finite(hh)));
        }
        y += h + row_gap;
    }
    Solution {
        width: finite(frame_w),
        height: finite(frame_h),
        rects,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deesainer_doc::Padding;

    fn child(w: f64, h: f64) -> Child {
        Child {
            width: w,
            height: h,
            horizontal: Sizing::Fixed,
            vertical: Sizing::Fixed,
            hug_width: w,
            hug_height: h,
            skip: false,
            limits: SizeLimits::default(),
        }
    }

    fn row(gap: f64, pad: f64) -> AutoLayout {
        AutoLayout {
            direction: LayoutDirection::Horizontal,
            gap,
            padding: Padding::uniform(pad),
            ..AutoLayout::default()
        }
    }

    const NO_REFLOW: Reflow<'static> = &|_, _| 0.0;

    #[test]
    fn horizontal_stack_with_gap_and_padding() {
        let s = solve(
            &row(10.0, 5.0),
            200.0,
            50.0,
            &[child(20.0, 10.0), child(30.0, 20.0)],
            NO_REFLOW,
        );
        assert_eq!(s.rects[0], Some(Rect::new(5.0, 5.0, 20.0, 10.0)));
        assert_eq!(s.rects[1], Some(Rect::new(35.0, 5.0, 30.0, 20.0)));
        assert_eq!((s.width, s.height), (200.0, 50.0));
    }

    #[test]
    fn hug_frame_wraps_its_content() {
        let mut l = row(10.0, 5.0);
        l.primary_sizing = Sizing::Hug;
        l.counter_sizing = Sizing::Hug;
        let s = solve(
            &l,
            999.0,
            999.0,
            &[child(20.0, 10.0), child(30.0, 20.0)],
            NO_REFLOW,
        );
        assert_eq!((s.width, s.height), (5.0 + 20.0 + 10.0 + 30.0 + 5.0, 30.0));
    }

    #[test]
    fn fill_children_share_free_space() {
        let mut a = child(10.0, 10.0);
        a.horizontal = Sizing::Fill;
        let mut b = child(10.0, 10.0);
        b.horizontal = Sizing::Fill;
        let s = solve(
            &row(0.0, 0.0),
            100.0,
            10.0,
            &[a, child(20.0, 10.0), b],
            NO_REFLOW,
        );
        assert_eq!(s.rects[0].map(|r| r.width), Some(40.0));
        assert_eq!(s.rects[1].map(|r| r.x), Some(40.0));
        assert_eq!(s.rects[2].map(|r| r.x), Some(60.0));
    }

    #[test]
    fn alignment_and_space_between() {
        let mut l = row(0.0, 0.0);
        l.primary_align = PrimaryAlign::Center;
        l.counter_align = CounterAlign::Center;
        let s = solve(&l, 100.0, 50.0, &[child(20.0, 10.0)], NO_REFLOW);
        assert_eq!(s.rects[0], Some(Rect::new(40.0, 20.0, 20.0, 10.0)));
        l.primary_align = PrimaryAlign::SpaceBetween;
        let s = solve(
            &l,
            100.0,
            50.0,
            &[child(20.0, 10.0), child(20.0, 10.0), child(20.0, 10.0)],
            NO_REFLOW,
        );
        assert_eq!(s.rects[1].map(|r| r.x), Some(40.0));
        assert_eq!(s.rects[2].map(|r| r.x), Some(80.0));
        l.primary_align = PrimaryAlign::End;
        l.counter_align = CounterAlign::End;
        let s = solve(&l, 100.0, 50.0, &[child(20.0, 10.0)], NO_REFLOW);
        assert_eq!(s.rects[0], Some(Rect::new(80.0, 40.0, 20.0, 10.0)));
    }

    #[test]
    fn vertical_stack_with_cross_fill_and_reflow() {
        let l = AutoLayout {
            direction: LayoutDirection::Vertical,
            gap: 8.0,
            padding: Padding::uniform(10.0),
            primary_sizing: Sizing::Hug,
            ..AutoLayout::default()
        };
        let mut text = child(50.0, 20.0);
        text.horizontal = Sizing::Fill;
        text.vertical = Sizing::Hug;
        let reflow: Reflow<'_> = &|_, w| 1000.0 / w;
        let s = solve(&l, 120.0, 0.0, &[text, child(40.0, 30.0)], reflow);
        assert_eq!(s.rects[0], Some(Rect::new(10.0, 10.0, 100.0, 10.0)));
        assert_eq!(s.rects[1], Some(Rect::new(10.0, 28.0, 40.0, 30.0)));
        assert_eq!(s.height, 10.0 + 10.0 + 8.0 + 30.0 + 10.0);
    }

    #[test]
    fn skipped_children_keep_none_and_take_no_space() {
        let mut abs = child(10.0, 10.0);
        abs.skip = true;
        let s = solve(
            &row(10.0, 0.0),
            100.0,
            10.0,
            &[abs, child(10.0, 10.0)],
            NO_REFLOW,
        );
        assert_eq!(s.rects[0], None);
        assert_eq!(s.rects[1].map(|r| r.x), Some(0.0));
    }

    #[test]
    fn horizontal_wrap_breaks_lines() {
        let mut l = row(10.0, 0.0);
        l.wrap = true;
        l.counter_gap = 5.0;
        l.counter_sizing = Sizing::Hug;
        let kids = [child(40.0, 10.0), child(40.0, 20.0), child(40.0, 10.0)];
        let s = solve(&l, 100.0, 0.0, &kids, NO_REFLOW);
        assert_eq!(s.rects[1].map(|r| r.x), Some(50.0));
        assert_eq!(s.rects[2], Some(Rect::new(0.0, 25.0, 40.0, 10.0)));
        assert_eq!(s.height, 20.0 + 5.0 + 10.0);
    }

    #[test]
    fn hostile_numbers_stay_finite() {
        let mut l = row(f64::NAN, 0.0);
        l.primary_sizing = Sizing::Hug;
        let s = solve(
            &l,
            f64::INFINITY,
            -5.0,
            &[child(f64::INFINITY, f64::NAN)],
            NO_REFLOW,
        );
        assert!(s.width.is_finite() && s.height.is_finite());
    }

    fn fill(min: Option<f64>, max: Option<f64>) -> Child {
        Child {
            horizontal: Sizing::Fill,
            limits: SizeLimits {
                min_width: min,
                max_width: max,
                ..SizeLimits::default()
            },
            ..child(0.0, 10.0)
        }
    }

    #[test]
    fn fill_stops_at_its_maximum_and_the_others_take_the_rest() {
        let s = solve(
            &row(0.0, 0.0),
            300.0,
            10.0,
            &[fill(None, Some(40.0)), fill(None, None), fill(None, None)],
            NO_REFLOW,
        );
        let w: Vec<f64> = s.rects.iter().map(|r| r.unwrap().width).collect();
        assert_eq!(w, [40.0, 130.0, 130.0]);
        let s = solve(
            &row(0.0, 0.0),
            100.0,
            10.0,
            &[fill(Some(80.0), None), fill(None, None)],
            NO_REFLOW,
        );
        let w: Vec<f64> = s.rects.iter().map(|r| r.unwrap().width).collect();
        assert_eq!(w, [80.0, 20.0]);
    }

    #[test]
    fn limits_hold_fixed_and_hug_children_and_hugging_frames() {
        let mut c = child(200.0, 10.0);
        c.limits.max_width = Some(120.0);
        c.limits.min_height = Some(30.0);
        let mut l = row(0.0, 0.0);
        l.primary_sizing = Sizing::Hug;
        l.counter_sizing = Sizing::Hug;
        let s = solve(&l, 0.0, 0.0, &[c], NO_REFLOW);
        assert_eq!(s.rects[0], Some(Rect::new(0.0, 0.0, 120.0, 30.0)));
        let frame = SizeLimits {
            max_width: Some(100.0),
            min_height: Some(50.0),
            ..SizeLimits::default()
        };
        let s = solve_limited(&l, 0.0, 0.0, &[c], frame, NO_REFLOW);
        assert_eq!((s.width, s.height), (100.0, 50.0));
    }
}

pub fn smart_animate(
    doc: &deesainer_doc::Document,
    from: deesainer_doc::NodeId,
    to: deesainer_doc::NodeId,
    t: f64,
) -> deesainer_doc::Document {
    use deesainer_doc::{NodeKind, Paint, PaintKind};
    let t = t.clamp(0.0, 1.0);
    let mut out = doc.clone();
    let names = |root: deesainer_doc::NodeId| -> Vec<(deesainer_doc::NodeId, String)> {
        doc.subtree_pre_order(root)
            .unwrap_or_default()
            .into_iter()
            .skip(1)
            .filter_map(|id| doc.get(id).map(|n| (id, n.name.clone())))
            .collect()
    };
    let from_layers = names(from);
    for (id, name) in names(to) {
        let Some(target) = doc.get(id) else { continue };
        let Some(node) = out.get_mut(id) else {
            continue;
        };
        match from_layers
            .iter()
            .find(|(_, n)| *n == name)
            .and_then(|(fid, _)| doc.get(*fid))
        {
            Some(src) => {
                node.rect = src.rect.lerp(target.rect, t);
                node.rotation = src.rotation + (target.rotation - src.rotation) * t;
                node.opacity = src.opacity + (target.opacity - src.opacity) * t as f32;
                node.corner_radius.top_left = src.corner_radius.top_left
                    + (target.corner_radius.top_left - src.corner_radius.top_left) * t;
                node.corner_radius.top_right = src.corner_radius.top_right
                    + (target.corner_radius.top_right - src.corner_radius.top_right) * t;
                node.corner_radius.bottom_right = src.corner_radius.bottom_right
                    + (target.corner_radius.bottom_right - src.corner_radius.bottom_right) * t;
                node.corner_radius.bottom_left = src.corner_radius.bottom_left
                    + (target.corner_radius.bottom_left - src.corner_radius.bottom_left) * t;
                if let (
                    Some(Paint {
                        kind: PaintKind::Solid { color: a },
                        ..
                    }),
                    Some(Paint {
                        kind: PaintKind::Solid { color: b },
                        ..
                    }),
                ) = (src.fills.first(), target.fills.first())
                {
                    if let Some(p) = node.fills.first_mut() {
                        p.kind = PaintKind::Solid {
                            color: a.lerp(*b, t as f32),
                        };
                    }
                } else if t < 0.5 {
                    node.fills = src.fills.clone();
                }
                if let (NodeKind::Text(_), NodeKind::Text(_)) = (&src.kind, &target.kind) {
                    if t < 0.5 {
                        node.kind = src.kind.clone();
                    }
                }
            }
            None => {
                node.opacity = target.opacity * t as f32;
            }
        }
    }
    out
}

#[cfg(test)]
mod smart_animate_tests {
    use super::*;
    use deesainer_color::Rgba;
    use deesainer_doc::{Document, Node, NodeKind, Paint};

    #[test]
    fn matching_layers_interpolate_and_new_ones_fade() {
        let mut doc = Document::new();
        let page = doc.first_page();
        let mut ids = Vec::new();
        for (x, frame_name) in [(0.0, "A"), (500.0, "B")] {
            let f = Node::new(
                doc.next_free_id(),
                frame_name,
                NodeKind::Frame,
                Rect::new(x, 0.0, 300.0, 300.0),
            );
            let fid = f.id;
            doc.insert(f, page, 0).unwrap();
            ids.push(fid);
        }
        let mut a = Node::new(
            doc.next_free_id(),
            "Box",
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        a.fills.push(Paint::solid(Rgba::BLACK));
        doc.insert(a, ids[0], 0).unwrap();
        let mut b = Node::new(
            doc.next_free_id(),
            "Box",
            NodeKind::Rectangle,
            Rect::new(100.0, 0.0, 30.0, 10.0),
        );
        b.fills.push(Paint::solid(Rgba::WHITE));
        let bid = b.id;
        doc.insert(b, ids[1], 0).unwrap();
        let extra = Node::new(
            doc.next_free_id(),
            "Extra",
            NodeKind::Rectangle,
            Rect::new(0.0, 0.0, 5.0, 5.0),
        );
        let eid = extra.id;
        doc.insert(extra, ids[1], 1).unwrap();
        let mid = smart_animate(&doc, ids[0], ids[1], 0.5);
        assert_eq!(mid.get(bid).unwrap().rect, Rect::new(50.0, 0.0, 20.0, 10.0));
        assert_eq!(mid.get(eid).unwrap().opacity, 0.5);
        let color = mid.get(bid).unwrap().fills[0].primary_color().unwrap();
        assert!((color.r - 0.5).abs() < 1e-6);
    }
}
