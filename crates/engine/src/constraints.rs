use deesainer_doc::{Constraint, Document, NodeId, NodeKind, PropValue};
use deesainer_geom::Rect;
use deesainer_ops::Op;

use crate::error::EngineError;

pub fn solve_axis(c: Constraint, pos: f64, size: f64, old: f64, new: f64) -> (f64, f64) {
    c.solve(pos, size, old, new)
}

pub fn child_ops(
    doc: &Document,
    parent: NodeId,
    old: Rect,
    new: Rect,
) -> Result<Vec<Op>, EngineError> {
    let mut ops = Vec::new();
    let mut stack = vec![(parent, old, new, 0usize)];
    while let Some((pid, old, new, depth)) = stack.pop() {
        if depth > deesainer_doc::MAX_DEPTH {
            break;
        }
        if (old.width - new.width).abs() < 1e-9 && (old.height - new.height).abs() < 1e-9 {
            continue;
        }
        let pnode = doc.node(pid)?;
        if pnode.layout.is_some() {
            continue;
        }
        let group_like = matches!(pnode.kind, NodeKind::Group | NodeKind::BooleanOp { .. });
        if !pnode.kind.is_container() || matches!(pnode.kind, NodeKind::Page | NodeKind::Document) {
            continue;
        }
        for cid in &pnode.children {
            let child = doc.node(*cid)?;
            let (h, v) = if group_like {
                (Constraint::Scale, Constraint::Scale)
            } else {
                (child.constraints.horizontal, child.constraints.vertical)
            };
            let (x, w) = solve_axis(h, child.rect.x, child.rect.width, old.width, new.width);
            let (y, hgt) = solve_axis(v, child.rect.y, child.rect.height, old.height, new.height);
            let rect = Rect::new(x, y, w, hgt);
            if !rect.is_valid() || rect == child.rect {
                continue;
            }
            ops.push(Op::set(*cid, PropValue::Rect(rect)));
            if let NodeKind::Vector { path } = &child.kind {
                let scaled = path.rescale(child.rect.width, child.rect.height, w, hgt);
                ops.push(Op::set(
                    *cid,
                    PropValue::Kind(NodeKind::Vector { path: scaled }),
                ));
            }
            stack.push((*cid, child.rect, rect, depth + 1));
        }
    }
    Ok(ops)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_rules() {
        assert_eq!(
            solve_axis(Constraint::Min, 10.0, 20.0, 100.0, 200.0),
            (10.0, 20.0)
        );
        assert_eq!(
            solve_axis(Constraint::Max, 10.0, 20.0, 100.0, 200.0),
            (110.0, 20.0)
        );
        assert_eq!(
            solve_axis(Constraint::Center, 40.0, 20.0, 100.0, 200.0),
            (90.0, 20.0)
        );
        assert_eq!(
            solve_axis(Constraint::Stretch, 10.0, 20.0, 100.0, 200.0),
            (10.0, 120.0)
        );
        assert_eq!(
            solve_axis(Constraint::Scale, 10.0, 20.0, 100.0, 200.0),
            (20.0, 40.0)
        );
    }
}
