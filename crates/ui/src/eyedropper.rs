use deesainer_color::Rgba;
use deesainer_doc::{NodeId, Paint, PaintKind};
use deesainer_geom::{Affine, Point};
use serde_json::json;

use crate::App;

pub(crate) fn sample(app: &App, p: Point) -> Option<Rgba> {
    if !p.is_finite() {
        return None;
    }
    let vp = deesainer_render::Viewport {
        transform: Affine::translate(-p.x.floor(), -p.y.floor()),
        width: 1,
        height: 1,
    };
    let opts = deesainer_render::Options {
        background: true,
        text: Some(deesainer_engine::text_painter()),
        hidden: &[],
    };
    let (pix, _) =
        deesainer_render::render_page(app.session.doc(), app.session.current_page(), &vp, opts)
            .ok()?;
    let px = pix.data().get(..4)?;
    let a = px[3];
    if a == 0 {
        return Some(Rgba::from_rgba8([255, 255, 255, 255]));
    }
    let un = |c: u8| ((u16::from(c) * 255 + u16::from(a) / 2) / u16::from(a)).min(255) as u8;
    Some(Rgba::from_rgba8([un(px[0]), un(px[1]), un(px[2]), 255]))
}

pub(crate) fn with_color(fills: &[Paint], color: Rgba) -> Vec<Paint> {
    let mut out = fills.to_vec();
    match out
        .iter_mut()
        .rev()
        .find(|p| matches!(p.kind, PaintKind::Solid { .. }))
    {
        Some(p) => p.kind = PaintKind::Solid { color },
        None => out.push(Paint::solid(color)),
    }
    out
}

pub(crate) fn apply(app: &mut App, ids: &[NodeId], color: Rgba) {
    let steps: Vec<(NodeId, Vec<Paint>)> = ids
        .iter()
        .filter_map(|id| {
            app.session
                .doc()
                .get(*id)
                .map(|n| (*id, with_color(&n.fills, color)))
        })
        .collect();
    app.session.begin_group("Pick colour");
    for (id, fills) in steps {
        app.dispatch("node.setFills", json!({ "ids": [id], "fills": fills }));
    }
    app.session.end_group();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_top_solid_fill_takes_the_colour_and_keeps_its_opacity() {
        let red = Rgba::new(1.0, 0.0, 0.0, 1.0);
        let blue = Rgba::new(0.0, 0.0, 1.0, 1.0);
        let mut top = Paint::solid(Rgba::BLACK);
        top.opacity = 0.5;
        let fills = vec![Paint::solid(Rgba::WHITE), top];
        let out = with_color(&fills, red);
        assert_eq!(out[1].kind, PaintKind::Solid { color: red });
        assert_eq!(out[1].opacity, 0.5);
        assert_eq!(out[0], fills[0]);
        assert_eq!(with_color(&[], blue), vec![Paint::solid(blue)]);
    }

    #[test]
    fn sampling_reads_the_drawn_colour() {
        let mut app = App::new(deesainer_engine::Session::new().unwrap());
        app.session
            .dispatch(
                "node.create.rectangle",
                &json!({"x": 0, "y": 0, "width": 10, "height": 10, "fill": "#336699"}),
            )
            .unwrap();
        let c = sample(&app, Point::new(5.0, 5.0)).unwrap().to_rgba8();
        assert_eq!(c, [0x33, 0x66, 0x99, 255]);
        let bg = sample(&app, Point::new(50.0, 50.0)).unwrap().to_rgba8();
        assert_eq!(bg, [0xF5, 0xF5, 0xF5, 255]);
    }
}
