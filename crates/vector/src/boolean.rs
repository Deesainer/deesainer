use deesainer_doc::{BoolOp, FillRule, PathCmd, StrokeJoin, VectorPath};
use deesainer_geom::Point;
use i_overlay::core::fill_rule::FillRule as OverlayFill;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::simplify::SimplifyShape;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::style::{LineJoin, OutlineStyle};

type Contours = Vec<Vec<[f64; 2]>>;

fn to_contours(polys: &[Vec<Point>]) -> Contours {
    polys
        .iter()
        .filter(|p| p.len() >= 3)
        .map(|p| p.iter().map(|q| [q.x, q.y]).collect())
        .collect()
}

pub fn apply(op: BoolOp, operands: &[Vec<Vec<Point>>]) -> VectorPath {
    let mut iter = operands
        .iter()
        .map(|o| to_contours(o))
        .filter(|c| !c.is_empty());
    let Some(mut acc) = iter.next() else {
        return VectorPath::default();
    };
    let rule = match op {
        BoolOp::Union => OverlayRule::Union,
        BoolOp::Subtract => OverlayRule::Difference,
        BoolOp::Intersect => OverlayRule::Intersect,
        BoolOp::Exclude => OverlayRule::Xor,
    };
    for next in iter {
        let shapes = acc.overlay(&next, rule, OverlayFill::NonZero);
        acc = shapes.into_iter().flatten().collect();
    }
    to_path(&acc)
}

pub fn stroke_ring(
    polys: &[Vec<Point>],
    weight: f64,
    outside: bool,
    join: StrokeJoin,
    fill_rule: FillRule,
) -> VectorPath {
    let rule = match fill_rule {
        FillRule::NonZero => OverlayFill::NonZero,
        FillRule::EvenOdd => OverlayFill::EvenOdd,
    };
    let shape: Contours = to_contours(polys)
        .simplify_shape(rule)
        .into_iter()
        .flatten()
        .collect();
    if shape.is_empty() || !(weight > 0.0 && weight.is_finite()) {
        return VectorPath::default();
    }
    let join = match join {
        StrokeJoin::Miter => LineJoin::Miter(2.0 * 0.25f64.asin()),
        StrokeJoin::Round => LineJoin::Round(0.1),
        StrokeJoin::Bevel => LineJoin::Bevel,
    };
    let offset = if outside { weight } else { -weight };
    let style = OutlineStyle::new(offset).line_join(join);
    let moved: Contours = shape.outline(&style).into_iter().flatten().collect();
    let ring = if outside {
        moved.overlay(&shape, OverlayRule::Difference, OverlayFill::NonZero)
    } else {
        shape.overlay(&moved, OverlayRule::Difference, OverlayFill::NonZero)
    };
    let ring: Contours = ring.into_iter().flatten().collect();
    to_path(&ring)
}

pub fn normalize(polys: &[Vec<Point>], fill_rule: FillRule) -> Vec<Vec<Point>> {
    let rule = match fill_rule {
        FillRule::NonZero => OverlayFill::NonZero,
        FillRule::EvenOdd => OverlayFill::EvenOdd,
    };
    to_contours(polys)
        .simplify_shape(rule)
        .into_iter()
        .flatten()
        .map(|c| c.into_iter().map(|[x, y]| Point::new(x, y)).collect())
        .collect()
}

pub fn merged_outline(path: &VectorPath, tolerance: f64) -> VectorPath {
    let polys = crate::flatten(path, tolerance);
    to_path(&to_contours(&normalize(&polys, path.fill_rule)))
}

fn to_path(acc: &Contours) -> VectorPath {
    let mut cmds = Vec::new();
    for contour in acc {
        let mut first = true;
        for [x, y] in contour {
            let to = Point::new(*x, *y);
            cmds.push(if first {
                PathCmd::MoveTo { to }
            } else {
                PathCmd::LineTo { to }
            });
            first = false;
        }
        if !first {
            cmds.push(PathCmd::Close);
        }
    }
    VectorPath {
        cmds,
        fill_rule: FillRule::EvenOdd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contains, flatten};

    fn square(x: f64, y: f64, s: f64) -> Vec<Vec<Point>> {
        vec![vec![
            Point::new(x, y),
            Point::new(x + s, y),
            Point::new(x + s, y + s),
            Point::new(x, y + s),
        ]]
    }

    fn area(path: &VectorPath) -> f64 {
        flatten(path, 0.1)
            .iter()
            .map(|c| {
                let n = c.len();
                (0..n)
                    .map(|i| {
                        let (a, b) = (c[i], c[(i + 1) % n]);
                        a.x * b.y - b.x * a.y
                    })
                    .sum::<f64>()
                    / 2.0
            })
            .sum::<f64>()
            .abs()
    }

    #[test]
    fn stroke_rings_lie_inside_or_outside_the_shape() {
        let sq = square(0.0, 0.0, 100.0);
        let inside = stroke_ring(&sq, 10.0, false, StrokeJoin::Miter, FillRule::NonZero);
        assert!((area(&inside) - 3600.0).abs() < 1.0, "{}", area(&inside));
        let polys = flatten(&inside, 0.1);
        assert!(contains(&polys, Point::new(5.0, 50.0), FillRule::EvenOdd));
        assert!(!contains(&polys, Point::new(50.0, 50.0), FillRule::EvenOdd));
        let outside = stroke_ring(&sq, 10.0, true, StrokeJoin::Miter, FillRule::NonZero);
        assert!((area(&outside) - 4400.0).abs() < 1.0, "{}", area(&outside));
        let polys = flatten(&outside, 0.1);
        assert!(contains(&polys, Point::new(-5.0, 50.0), FillRule::EvenOdd));
        assert!(!contains(&polys, Point::new(5.0, 50.0), FillRule::EvenOdd));
        let round = stroke_ring(&sq, 10.0, true, StrokeJoin::Round, FillRule::NonZero);
        let exact = 4000.0 + std::f64::consts::PI * 100.0;
        assert!((area(&round) - exact).abs() < 10.0, "{}", area(&round));
        assert!(
            stroke_ring(&sq, 0.0, true, StrokeJoin::Miter, FillRule::NonZero)
                .cmds
                .is_empty()
        );
    }

    #[test]
    fn union_intersect_subtract_exclude() {
        let a = square(0.0, 0.0, 10.0);
        let b = square(5.0, 5.0, 10.0);
        let probe = |op, p: Point| {
            let path = apply(op, &[a.clone(), b.clone()]);
            contains(&flatten(&path, 0.1), p, path.fill_rule)
        };
        let both = Point::new(7.0, 7.0);
        let only_a = Point::new(2.0, 2.0);
        let only_b = Point::new(13.0, 13.0);
        assert!(
            probe(BoolOp::Union, both)
                && probe(BoolOp::Union, only_a)
                && probe(BoolOp::Union, only_b)
        );
        assert!(probe(BoolOp::Intersect, both) && !probe(BoolOp::Intersect, only_a));
        assert!(
            !probe(BoolOp::Subtract, both)
                && probe(BoolOp::Subtract, only_a)
                && !probe(BoolOp::Subtract, only_b)
        );
        assert!(
            !probe(BoolOp::Exclude, both)
                && probe(BoolOp::Exclude, only_a)
                && probe(BoolOp::Exclude, only_b)
        );
    }

    #[test]
    fn empty_operands_give_empty_path() {
        assert!(apply(BoolOp::Union, &[]).cmds.is_empty());
    }
}

#[cfg(test)]
mod merged_tests {
    use super::*;

    fn square(x: f64) -> [PathCmd; 5] {
        [
            PathCmd::MoveTo {
                to: Point::new(x, 0.0),
            },
            PathCmd::LineTo {
                to: Point::new(x + 10.0, 0.0),
            },
            PathCmd::LineTo {
                to: Point::new(x + 10.0, 10.0),
            },
            PathCmd::LineTo {
                to: Point::new(x, 10.0),
            },
            PathCmd::Close,
        ]
    }

    #[test]
    fn overlapping_contours_merge_into_one_outline() {
        let mut path = VectorPath::default();
        path.cmds.extend(square(0.0));
        path.cmds.extend(square(5.0));
        let merged = merged_outline(&path, 0.1);
        let moves = merged
            .cmds
            .iter()
            .filter(|c| matches!(c, PathCmd::MoveTo { .. }))
            .count();
        assert_eq!(moves, 1);
        let b = crate::path_bounds(&merged).unwrap();
        assert_eq!((b.width, b.height), (15.0, 10.0));
    }
}
