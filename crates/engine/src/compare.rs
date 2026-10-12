use deesainer_doc::{Document, NodeId};
use deesainer_render::Pixmap;

pub struct Comparison {
    pub before: Pixmap,
    pub after: Pixmap,
    pub diff: Pixmap,
    pub changed: u64,
}

fn page_content(doc: &Document, page: NodeId) -> Option<deesainer_geom::Rect> {
    doc.get(page)?
        .children
        .iter()
        .filter_map(|c| deesainer_render::render_bounds(doc, *c))
        .reduce(deesainer_geom::Rect::union)
}

pub fn documents(
    then: &Document,
    now: &Document,
    index: usize,
    scale: f64,
) -> Result<Comparison, String> {
    let then_page = then.pages().get(index).copied();
    let now_page = now.pages().get(index).copied();
    if then_page.is_none() && now_page.is_none() {
        return Err("neither document has that page".into());
    }
    let area = [
        then_page.and_then(|pg| page_content(then, pg)),
        now_page.and_then(|pg| page_content(now, pg)),
    ]
    .into_iter()
    .flatten()
    .reduce(deesainer_geom::Rect::union)
    .unwrap_or(deesainer_geom::Rect::new(0.0, 0.0, 1.0, 1.0));
    let scale = scale
        .clamp(0.05, 4.0)
        .min(4096.0 / area.width.max(area.height).max(1.0));
    let (w, h) = (
        (area.width * scale).ceil().max(1.0) as u32,
        (area.height * scale).ceil().max(1.0) as u32,
    );
    let vp = deesainer_render::Viewport {
        transform: deesainer_geom::Affine::scale(scale, scale)
            .then(deesainer_geom::Affine::translate(-area.x, -area.y)),
        width: w,
        height: h,
    };
    let draw = |doc: &Document, page: Option<NodeId>| -> Result<Pixmap, String> {
        let opts = deesainer_render::Options {
            background: true,
            text: Some(crate::text_painter()),
            hidden: &[],
        };
        match page {
            Some(pg) => deesainer_render::render_page(doc, pg, &vp, opts)
                .map(|(pix, _)| pix)
                .map_err(|e| e.to_string()),
            None => Pixmap::new(w, h).ok_or_else(|| "empty area".to_owned()),
        }
    };
    let before = draw(then, then_page)?;
    let after = draw(now, now_page)?;
    let mut diff = after.clone();
    let mut changed = 0u64;
    for (d, (a, b)) in diff.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(
        before
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .zip(after.data().as_chunks::<4>().0),
    ) {
        let delta = a
            .iter()
            .zip(b)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        if delta > 8 {
            changed += 1;
            d.copy_from_slice(&[255, 0, 200, 255]);
        } else {
            for c in d.iter_mut().take(3) {
                *c = (u16::from(*c) / 3 + 170) as u8;
            }
            d[3] = 255;
        }
    }
    Ok(Comparison {
        before,
        after,
        diff,
        changed,
    })
}
