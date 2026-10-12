#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use deesainer_color::{BlendMode, Rgba};
use deesainer_doc::{ColorStop, Document, Node, NodeId, NodeKind, Paint};
use deesainer_geom::{Affine, Point, Rect};
use deesainer_gpu::{Compositor, Frame, View};

const W: u32 = 200;
const H: u32 = 120;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
        return None;
    }
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

fn add(doc: &mut Document, parent: NodeId, id: u64, kind: NodeKind, rect: Rect) -> NodeId {
    let at = doc.get(parent).map_or(0, |p| p.children.len());
    doc.insert(
        Node::new(NodeId(id + 100), format!("n{id}"), kind, rect),
        parent,
        at,
    )
    .unwrap();
    NodeId(id + 100)
}

fn solid(r: f32, g: f32, b: f32, a: f32) -> Paint {
    Paint::solid(Rgba::new(r, g, b, a))
}

fn page() -> (Document, NodeId) {
    let mut doc = Document::new();
    let page = doc.first_page();
    doc.get_mut(page).unwrap().fills = vec![solid(1.0, 1.0, 1.0, 1.0)];
    let back = add(
        &mut doc,
        page,
        1,
        NodeKind::Rectangle,
        Rect::new(10.0, 10.0, 180.0, 100.0),
    );
    doc.get_mut(back).unwrap().fills = vec![Paint::linear(
        Point::new(0.0, 0.5),
        Point::new(1.0, 0.5),
        vec![
            ColorStop {
                position: 0.0,
                color: Rgba::new(0.9, 0.2, 0.1, 1.0),
            },
            ColorStop {
                position: 1.0,
                color: Rgba::new(0.1, 0.4, 0.95, 1.0),
            },
        ],
    )];
    (doc, page)
}

fn cpu(doc: &Document, page: NodeId) -> Vec<u8> {
    cpu_t(doc, page, None)
}

fn cpu_t(
    doc: &Document,
    page: NodeId,
    text: Option<&dyn deesainer_render::TextPainter>,
) -> Vec<u8> {
    let vp = deesainer_render::Viewport {
        transform: Affine::IDENTITY,
        width: W,
        height: H,
    };
    let opts = deesainer_render::Options {
        background: true,
        text,
        hidden: &[],
    };
    let (pix, _) = deesainer_render::render_page(doc, page, &vp, opts).unwrap();
    pix.data().to_vec()
}

fn gpu(device: &wgpu::Device, queue: &wgpu::Queue, doc: &Document, page: NodeId) -> Vec<u8> {
    gpu_t(device, queue, doc, page, None)
}

fn gpu_t(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    doc: &Document,
    page: NodeId,
    text: Option<&dyn deesainer_render::TextPainter>,
) -> Vec<u8> {
    let mesh = deesainer_mesh::build(
        doc,
        page,
        Rect::new(-10.0, -10.0, f64::from(W) + 20.0, f64::from(H) + 20.0),
        1.0,
        text,
    )
    .unwrap();
    let textures = mesh
        .gradients
        .iter()
        .map(|(key, pixels)| (deesainer_mesh::TextureRef::Gradient(*key), pixels.clone()))
        .collect();
    let frame = Frame {
        background: mesh.background.unwrap_or([0; 4]),
        mesh: Arc::new(mesh),
        mesh_id: 1,
        textures: Arc::new(textures),
        view: View {
            scale: 1.0,
            offset: [0.0, 0.0],
            size: [W, H],
        },
    };
    let mut c = Compositor::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, 1);
    c.render_to_pixels(device, queue, &frame).unwrap()
}

fn compare(a: &[u8], b: &[u8]) -> (f64, f64) {
    assert_eq!(a.len(), b.len());
    let mut sum = 0u64;
    let mut off = 0usize;
    for (pa, pb) in a.chunks(4).zip(b.chunks(4)) {
        let d = pa
            .iter()
            .zip(pb)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        sum += u64::from(d);
        if d > 8 {
            off += 1;
        }
    }
    let n = (a.len() / 4) as f64;
    (sum as f64 / n, off as f64 / n)
}

fn check(name: &str, doc: &Document, page: NodeId) {
    let Some((device, queue)) = device() else {
        eprintln!("{name}: no hardware GPU adapter on this machine; the oracle cannot run");
        return;
    };
    let (mean, off) = compare(&cpu(doc, page), &gpu(&device, &queue, doc, page));
    eprintln!(
        "{name}: mean difference {mean:.3}, {:.2}% pixels off",
        off * 100.0
    );
    assert!(mean < 1.5, "{name}: mean difference {mean}");
    assert!(off < 0.03, "{name}: {:.2}% pixels off", off * 100.0);
}

#[test]
fn plain_shapes_match() {
    let (mut doc, page) = page();
    let r = add(
        &mut doc,
        page,
        2,
        NodeKind::ellipse(),
        Rect::new(40.0, 20.0, 80.0, 80.0),
    );
    doc.get_mut(r).unwrap().fills = vec![solid(0.1, 0.8, 0.3, 0.7)];
    check("plain shapes", &doc, page);
}

#[test]
fn group_opacity_matches() {
    let (mut doc, page) = page();
    let group = add(
        &mut doc,
        page,
        2,
        NodeKind::Frame,
        Rect::new(20.0, 20.0, 150.0, 80.0),
    );
    {
        let g = doc.get_mut(group).unwrap();
        g.opacity = 0.5;
        g.clips_content = false;
        g.fills = Vec::new();
    }
    for (i, x) in [0.0, 50.0].into_iter().enumerate() {
        let c = add(
            &mut doc,
            group,
            3 + i as u64,
            NodeKind::Rectangle,
            Rect::new(x, 10.0, 90.0, 60.0),
        );
        doc.get_mut(c).unwrap().fills = vec![solid(0.0, 0.0, 0.0, 1.0)];
    }
    check("group opacity", &doc, page);
}

#[test]
fn blend_modes_match() {
    for mode in [
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ] {
        let (mut doc, page) = page();
        let r = add(
            &mut doc,
            page,
            2,
            NodeKind::Rectangle,
            Rect::new(30.0, 25.0, 140.0, 70.0),
        );
        let n = doc.get_mut(r).unwrap();
        n.fills = vec![solid(0.95, 0.75, 0.2, 0.85)];
        n.blend = mode;
        check(&format!("layer blend {mode:?}"), &doc, page);
    }
}

#[test]
fn paint_blend_modes_match() {
    let (mut doc, page) = page();
    let r = add(
        &mut doc,
        page,
        2,
        NodeKind::Rectangle,
        Rect::new(30.0, 25.0, 140.0, 70.0),
    );
    let mut multiply = solid(0.2, 0.9, 0.6, 1.0);
    multiply.blend = BlendMode::Multiply;
    doc.get_mut(r).unwrap().fills = vec![solid(0.95, 0.75, 0.2, 1.0), multiply];
    check("paint blend", &doc, page);
}

#[test]
fn color_blend_follows_the_specification() {
    let Some((device, queue)) = device() else {
        eprintln!("color blend: no GPU adapter on this machine; the test cannot run");
        return;
    };
    let (mut doc, page) = page();
    let r = add(
        &mut doc,
        page,
        2,
        NodeKind::Rectangle,
        Rect::new(30.0, 25.0, 140.0, 70.0),
    );
    let n = doc.get_mut(r).unwrap();
    n.fills = vec![solid(0.95, 0.75, 0.2, 0.85)];
    n.blend = BlendMode::Color;
    let backdrop = {
        let (doc, page) = self::page();
        cpu(&doc, page)
    };
    let out = gpu(&device, &queue, &doc, page);
    let lum = |c: [f64; 3]| 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
    let set_lum = |c: [f64; 3], l: f64| {
        let d = l - lum(c);
        let c = c.map(|v| v + d);
        let l = lum(c);
        let (n, x) = (c[0].min(c[1]).min(c[2]), c[0].max(c[1]).max(c[2]));
        let c = if n < 0.0 {
            c.map(|v| l + (v - l) * l / (l - n))
        } else {
            c
        };
        if x > 1.0 {
            c.map(|v| l + (v - l) * (1.0 - l) / (x - l))
        } else {
            c
        }
    };
    for x in [40usize, 100, 160] {
        let i = (60 * W as usize + x) * 4;
        let cb = [0, 1, 2].map(|k| f64::from(backdrop[i + k]) / 255.0);
        let blended = set_lum([0.95, 0.75, 0.2], lum(cb));
        for k in 0..3 {
            let want = (0.85 * blended[k] + 0.15 * cb[k]) * 255.0;
            let got = f64::from(out[i + k]);
            assert!(
                (want - got).abs() <= 2.0,
                "x {x} channel {k}: want {want:.1}, got {got}"
            );
        }
    }
}

#[test]
fn nested_layers_keep_their_order() {
    let (mut doc, page) = page();
    let group = add(
        &mut doc,
        page,
        2,
        NodeKind::Frame,
        Rect::new(15.0, 15.0, 170.0, 90.0),
    );
    {
        let g = doc.get_mut(group).unwrap();
        g.opacity = 0.6;
        g.clips_content = false;
        g.fills = Vec::new();
    }
    let a = add(
        &mut doc,
        group,
        3,
        NodeKind::Rectangle,
        Rect::new(0.0, 0.0, 90.0, 60.0),
    );
    doc.get_mut(a).unwrap().fills = vec![solid(0.1, 0.9, 0.3, 1.0)];
    let b = add(
        &mut doc,
        group,
        4,
        NodeKind::ellipse(),
        Rect::new(40.0, 20.0, 80.0, 60.0),
    );
    let n = doc.get_mut(b).unwrap();
    n.fills = vec![solid(0.9, 0.2, 0.8, 1.0)];
    n.blend = BlendMode::Multiply;
    let c = add(
        &mut doc,
        group,
        5,
        NodeKind::Rectangle,
        Rect::new(100.0, 40.0, 60.0, 40.0),
    );
    doc.get_mut(c).unwrap().fills = vec![solid(0.95, 0.9, 0.1, 1.0)];
    let d = add(
        &mut doc,
        page,
        6,
        NodeKind::ellipse(),
        Rect::new(120.0, 5.0, 60.0, 60.0),
    );
    doc.get_mut(d).unwrap().fills = vec![solid(0.0, 0.3, 0.9, 0.8)];
    check("nested layers", &doc, page);
}

fn clipping_frame(doc: &mut Document, page: NodeId, id: u64) -> NodeId {
    let frame = add(
        doc,
        page,
        id,
        NodeKind::Frame,
        Rect::new(40.0, 20.0, 120.0, 80.0),
    );
    {
        let f = doc.get_mut(frame).unwrap();
        f.clips_content = true;
        f.fills = vec![solid(1.0, 1.0, 1.0, 0.3)];
    }
    let a = add(
        doc,
        frame,
        id + 1,
        NodeKind::Rectangle,
        Rect::new(-30.0, -20.0, 100.0, 140.0),
    );
    doc.get_mut(a).unwrap().fills = vec![solid(0.1, 0.2, 0.3, 1.0)];
    let b = add(
        doc,
        frame,
        id + 2,
        NodeKind::ellipse(),
        Rect::new(60.0, -10.0, 100.0, 100.0),
    );
    doc.get_mut(b).unwrap().fills = vec![solid(0.95, 0.6, 0.1, 0.9)];
    frame
}

#[test]
fn rounded_clips_match() {
    let (mut doc, page) = page();
    let frame = clipping_frame(&mut doc, page, 2);
    doc.get_mut(frame).unwrap().corner_radius = deesainer_doc::CornerRadius::uniform(24.0);
    check("rounded clip", &doc, page);
}

#[test]
fn rotated_clips_match() {
    let (mut doc, page) = page();
    let frame = clipping_frame(&mut doc, page, 2);
    doc.get_mut(frame).unwrap().rotation = 20.0;
    check("rotated clip", &doc, page);
}

#[test]
fn masks_match() {
    let (mut doc, page) = page();
    let mask = add(
        &mut doc,
        page,
        2,
        NodeKind::ellipse(),
        Rect::new(30.0, 15.0, 140.0, 90.0),
    );
    {
        let m = doc.get_mut(mask).unwrap();
        m.is_mask = true;
        m.fills = vec![solid(0.0, 0.0, 0.0, 0.6)];
    }
    let a = add(
        &mut doc,
        page,
        3,
        NodeKind::Rectangle,
        Rect::new(0.0, 0.0, 120.0, 120.0),
    );
    doc.get_mut(a).unwrap().fills = vec![solid(0.1, 0.8, 0.4, 1.0)];
    let b = add(
        &mut doc,
        page,
        4,
        NodeKind::Rectangle,
        Rect::new(100.0, 30.0, 100.0, 60.0),
    );
    doc.get_mut(b).unwrap().fills = vec![solid(0.9, 0.1, 0.5, 1.0)];
    check("mask", &doc, page);
}

fn card(doc: &mut Document, page: NodeId, id: u64) -> NodeId {
    let r = add(
        doc,
        page,
        id,
        NodeKind::Rectangle,
        Rect::new(50.0, 30.0, 100.0, 60.0),
    );
    let n = doc.get_mut(r).unwrap();
    n.fills = vec![solid(0.98, 0.95, 0.9, 1.0)];
    n.corner_radius = deesainer_doc::CornerRadius::uniform(12.0);
    r
}

#[test]
fn layer_blur_matches() {
    let (mut doc, page) = page();
    let r = card(&mut doc, page, 2);
    doc.get_mut(r).unwrap().effects = vec![deesainer_doc::Effect::LayerBlur {
        radius: 8.0,
        visible: true,
    }];
    check("layer blur", &doc, page);
}

#[test]
fn drop_shadows_match() {
    let (mut doc, page) = page();
    let r = card(&mut doc, page, 2);
    doc.get_mut(r).unwrap().effects = vec![deesainer_doc::Effect::DropShadow {
        color: Rgba::new(0.0, 0.0, 0.0, 0.45),
        offset: Point::new(6.0, 10.0),
        blur: 12.0,
        spread: 3.0,
        visible: true,
    }];
    check("drop shadow", &doc, page);
}

#[test]
fn inner_shadows_match() {
    let (mut doc, page) = page();
    let r = card(&mut doc, page, 2);
    doc.get_mut(r).unwrap().effects = vec![deesainer_doc::Effect::InnerShadow {
        color: Rgba::new(0.1, 0.0, 0.4, 0.7),
        offset: Point::new(4.0, 6.0),
        blur: 8.0,
        spread: 2.0,
        visible: true,
    }];
    check("inner shadow", &doc, page);
}

#[test]
fn background_blurs_match() {
    let (mut doc, page) = page();
    for i in 0..6 {
        let s = add(
            &mut doc,
            page,
            10 + i,
            NodeKind::Rectangle,
            Rect::new(20.0 + i as f64 * 28.0, 20.0, 14.0, 80.0),
        );
        doc.get_mut(s).unwrap().fills = vec![solid(1.0, 1.0, 1.0, 1.0)];
    }
    let r = card(&mut doc, page, 2);
    let n = doc.get_mut(r).unwrap();
    n.fills = vec![solid(1.0, 1.0, 1.0, 0.25)];
    n.effects = vec![deesainer_doc::Effect::BackgroundBlur {
        radius: 10.0,
        visible: true,
    }];
    let page_node = doc.get_mut(page).unwrap();
    page_node.children.retain(|c| *c != r);
    page_node.children.push(r);
    check("background blur", &doc, page);
}

#[test]
fn filters_work_with_pooled_targets_of_other_sizes() {
    let Some((device, queue)) = device() else {
        eprintln!("pooled targets: no GPU adapter on this machine; the test cannot run");
        return;
    };
    let mut compositor = Compositor::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, 1);
    let frame = |doc: &Document, page: NodeId, w: u32, h: u32| {
        let mesh = deesainer_mesh::build(
            doc,
            page,
            Rect::new(-10.0, -10.0, f64::from(w) + 20.0, f64::from(h) + 20.0),
            1.0,
            None,
        )
        .unwrap();
        Frame {
            background: mesh.background.unwrap_or([0; 4]),
            mesh: Arc::new(mesh),
            mesh_id: u64::from(w),
            textures: Arc::default(),
            view: View {
                scale: 1.0,
                offset: [0.0, 0.0],
                size: [w, h],
            },
        }
    };
    let (mut big, big_page) = page();
    let r = add(
        &mut big,
        big_page,
        2,
        NodeKind::Rectangle,
        Rect::new(0.0, 0.0, 600.0, 500.0),
    );
    let n = big.get_mut(r).unwrap();
    n.fills = vec![solid(0.2, 0.3, 0.4, 1.0)];
    n.blend = BlendMode::Multiply;
    compositor
        .render_to_pixels(&device, &queue, &frame(&big, big_page, 600, 500))
        .unwrap();
    let (mut doc, page) = page();
    let r = card(&mut doc, page, 2);
    doc.get_mut(r).unwrap().effects = vec![deesainer_doc::Effect::LayerBlur {
        radius: 8.0,
        visible: true,
    }];
    let out = compositor
        .render_to_pixels(&device, &queue, &frame(&doc, page, W, H))
        .unwrap();
    let (mean, off) = compare(&cpu(&doc, page), &out);
    assert!(
        mean < 1.5 && off < 0.03,
        "mean {mean}, {:.2}% off",
        off * 100.0
    );
}

#[test]
fn many_effect_layers_share_atlases() {
    let (mut doc, page) = page();
    let effects = |i: u64| -> Vec<deesainer_doc::Effect> {
        let shadow = |inner: bool| {
            let (color, offset, blur, spread, visible) = (
                Rgba::new(0.1, 0.0, 0.3, 0.6),
                Point::new(2.0, 3.0),
                4.0,
                1.0,
                true,
            );
            if inner {
                deesainer_doc::Effect::InnerShadow {
                    color,
                    offset,
                    blur,
                    spread,
                    visible,
                }
            } else {
                deesainer_doc::Effect::DropShadow {
                    color,
                    offset,
                    blur,
                    spread,
                    visible,
                }
            }
        };
        match i % 4 {
            0 => vec![shadow(false)],
            1 => vec![shadow(true)],
            2 => vec![deesainer_doc::Effect::LayerBlur {
                radius: 3.0,
                visible: true,
            }],
            _ => vec![shadow(false), shadow(true)],
        }
    };
    for i in 0..30u64 {
        let (col, row) = ((i % 6) as f64, (i / 6) as f64);
        let r = add(
            &mut doc,
            page,
            10 + i,
            NodeKind::Rectangle,
            Rect::new(8.0 + col * 32.0, 6.0 + row * 23.0, 24.0, 15.0),
        );
        let n = doc.get_mut(r).unwrap();
        n.fills = vec![solid(0.95, 0.9 - i as f32 * 0.02, 0.6, 1.0)];
        n.corner_radius = deesainer_doc::CornerRadius::uniform(4.0);
        n.effects = effects(i);
    }
    check("many effect layers", &doc, page);
}

struct Glyphs;

impl deesainer_render::TextPainter for Glyphs {
    fn outlines(&self, node: &Node) -> Vec<deesainer_doc::VectorPath> {
        deesainer_text::node_outlines(node)
    }

    fn parts(
        &self,
        node: &Node,
    ) -> (
        Vec<deesainer_doc::VectorPath>,
        Vec<(Paint, deesainer_doc::VectorPath)>,
    ) {
        let (base, colored) = deesainer_text::node_parts(node);
        (vec![base], colored)
    }
}

#[test]
fn text_strokes_match() {
    let Some((device, queue)) = device() else {
        eprintln!("text strokes: no GPU adapter on this machine; the test cannot run");
        return;
    };
    for dash in [Vec::new(), vec![3.0, 2.0]] {
        let (mut doc, page) = page();
        let data = deesainer_doc::TextData {
            content: "Stroke".into(),
            style: deesainer_doc::TextStyle {
                font_family: "sans-serif".into(),
                font_size: 48.0,
                ..deesainer_doc::TextStyle::default()
            },
            ..deesainer_doc::TextData::default()
        };
        let t = add(
            &mut doc,
            page,
            2,
            NodeKind::Text(data),
            Rect::new(20.0, 20.0, 170.0, 70.0),
        );
        let n = doc.get_mut(t).unwrap();
        n.fills = vec![solid(1.0, 1.0, 1.0, 1.0)];
        n.strokes = vec![solid(0.1, 0.1, 0.2, 1.0)];
        n.stroke.weight = 2.0;
        n.stroke.dash = dash.clone();
        let glyphs = Glyphs;
        let (c, g) = (
            cpu_t(&doc, page, Some(&glyphs)),
            gpu_t(&device, &queue, &doc, page, Some(&glyphs)),
        );
        let (mean, off) = compare(&c, &g);
        eprintln!(
            "text stroke {dash:?}: mean difference {mean:.3}, {:.2}% pixels off",
            off * 100.0
        );
        if dash.is_empty() {
            assert!(
                mean < 1.5 && off < 0.03,
                "text stroke: mean {mean}, {:.2}% off",
                off * 100.0
            );
        } else {
            let ink = |p: &[u8]| p.chunks(4).map(|q| u64::from(255 - q[0])).sum::<u64>() as f64;
            let (a, b) = (ink(&c), ink(&g));
            assert!(
                (a - b).abs() / a < 0.03,
                "dashed text ink: cpu {a}, gpu {b}"
            );
            assert!(mean < 3.5, "dashed text: mean {mean}");
        }
    }
}

#[test]
fn dashed_inside_and_outside_strokes_match() {
    let Some((device, queue)) = device() else {
        eprintln!("dashed strokes: no GPU adapter on this machine; the test cannot run");
        return;
    };
    for align in [
        deesainer_doc::StrokeAlign::Inside,
        deesainer_doc::StrokeAlign::Outside,
    ] {
        for kind in [NodeKind::Rectangle, NodeKind::ellipse()] {
            let straight = matches!(kind, NodeKind::Rectangle);
            let (mut doc, page) = page();
            let r = add(&mut doc, page, 2, kind, Rect::new(40.0, 25.0, 120.0, 70.0));
            let n = doc.get_mut(r).unwrap();
            n.fills = vec![solid(0.95, 0.95, 0.9, 1.0)];
            n.strokes = vec![solid(0.1, 0.1, 0.3, 1.0)];
            n.stroke.weight = 4.0;
            n.stroke.align = align;
            n.stroke.dash = vec![10.0, 6.0];
            let (c, g) = (cpu(&doc, page), gpu(&device, &queue, &doc, page));
            let (mean, off) = compare(&c, &g);
            eprintln!(
                "dashed {align:?} straight={straight}: mean {mean:.3}, {:.2}% off",
                off * 100.0
            );
            if straight {
                assert!(mean < 1.5 && off < 0.03, "dashed {align:?}: mean {mean}");
            } else {
                let ink = |p: &[u8]| p.chunks(4).map(|q| u64::from(255 - q[2])).sum::<u64>() as f64;
                let (a, b) = (ink(&c), ink(&g));
                assert!(
                    (a - b).abs() / a < 0.03,
                    "dashed {align:?} ink: cpu {a}, gpu {b}"
                );
                assert!(mean < 3.5, "dashed {align:?}: mean {mean}");
            }
        }
    }
}

fn deep(doc: &mut Document, page: NodeId) {
    let mut parent = page;
    for i in 0..20u64 {
        parent = add(
            doc,
            parent,
            100 + i,
            NodeKind::Frame,
            Rect::new(2.0, 2.0, 180.0, 100.0),
        );
        let f = doc.get_mut(parent).unwrap();
        f.opacity = 0.95;
        f.clips_content = false;
        f.fills = Vec::new();
    }
    let r = add(
        doc,
        parent,
        300,
        NodeKind::Rectangle,
        Rect::new(10.0, 5.0, 80.0, 40.0),
    );
    doc.get_mut(r).unwrap().fills = vec![solid(0.1, 0.2, 0.7, 1.0)];
}

#[test]
fn deeply_nested_layers_match() {
    let (mut doc, page) = page();
    deep(&mut doc, page);
    check("20 nested layers", &doc, page);
}

#[test]
fn layers_past_the_memory_budget_are_flattened_not_lost() {
    let Some((device, queue)) = device() else {
        eprintln!("memory budget: no GPU adapter on this machine; the test cannot run");
        return;
    };
    let (mut doc, page) = page();
    deep(&mut doc, page);
    let mesh = deesainer_mesh::build(&doc, page, Rect::new(-10.0, -10.0, 220.0, 140.0), 1.0, None)
        .unwrap();
    let textures = mesh
        .gradients
        .iter()
        .map(|(key, pixels)| (deesainer_mesh::TextureRef::Gradient(*key), pixels.clone()))
        .collect();
    let frame = Frame {
        background: mesh.background.unwrap_or([0; 4]),
        mesh: Arc::new(mesh),
        mesh_id: 1,
        textures: Arc::new(textures),
        view: View {
            scale: 1.0,
            offset: [0.0, 0.0],
            size: [W, H],
        },
    };
    let mut c = Compositor::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, 1);
    c.set_memory_budget(4 * 1024 * 1024);
    let out = c.render_to_pixels(&device, &queue, &frame).unwrap();
    let flattened = c.health().flattened_layers();
    assert!(flattened > 0, "nothing was flattened");
    assert!(c.health().failure().is_none());
    let i = (65 * W as usize + 90) * 4;
    assert!(
        out[i + 2] > out[i],
        "the blue shape is missing: {:?}",
        &out[i..i + 4]
    );
    eprintln!("memory budget: {flattened} layers flattened");
}

#[test]
fn slot_content_matches() {
    use deesainer_doc::{ComponentProp, ComponentPropKind, NodeKind as K};
    let (mut doc, page) = page();
    let main = add(
        &mut doc,
        page,
        2,
        K::Component {
            description: String::new(),
            properties: Vec::new(),
        },
        Rect::new(10.0, 10.0, 80.0, 60.0),
    );
    doc.get_mut(main).unwrap().fills = vec![solid(1.0, 1.0, 1.0, 1.0)];
    let slot = add(
        &mut doc,
        main,
        3,
        K::Frame,
        Rect::new(10.0, 10.0, 60.0, 40.0),
    );
    let default = add(
        &mut doc,
        slot,
        4,
        K::Rectangle,
        Rect::new(0.0, 0.0, 60.0, 40.0),
    );
    doc.get_mut(default).unwrap().fills = vec![solid(0.5, 0.5, 0.5, 1.0)];
    let target = doc.get(slot).unwrap().id;
    if let K::Component { properties, .. } = &mut doc.get_mut(main).unwrap().kind {
        properties.push(ComponentProp {
            name: "Body".into(),
            target,
            kind: ComponentPropKind::Slot {
                preferred: Vec::new(),
            },
        });
    }
    let a = add(
        &mut doc,
        page,
        5,
        K::Instance {
            component: main,
            overrides: Vec::new(),
        },
        Rect::new(100.0, 10.0, 80.0, 60.0),
    );
    let _b = add(
        &mut doc,
        page,
        6,
        K::Instance {
            component: main,
            overrides: Vec::new(),
        },
        Rect::new(100.0, 75.0, 80.0, 40.0),
    );
    let content = add(
        &mut doc,
        a,
        7,
        K::SlotContent {
            slot: "Body".into(),
        },
        Rect::new(10.0, 10.0, 60.0, 40.0),
    );
    let dot = add(
        &mut doc,
        content,
        8,
        K::ellipse(),
        Rect::new(5.0, 5.0, 30.0, 30.0),
    );
    doc.get_mut(dot).unwrap().fills = vec![solid(0.9, 0.2, 0.2, 1.0)];
    let px = cpu(&doc, page);
    let at = |x: usize, y: usize| &px[(y * W as usize + x) * 4..(y * W as usize + x) * 4 + 3];
    assert!(
        at(130, 40)[0] > 200 && at(130, 40)[1] < 80,
        "red dot: {:?}",
        at(130, 40)
    );
    assert!(
        at(130, 95)[0] < 160,
        "the other instance keeps the grey default: {:?}",
        at(130, 95)
    );
    check("slot content", &doc, page);
}

fn ramp() -> Vec<ColorStop> {
    vec![
        ColorStop {
            position: 0.0,
            color: Rgba::new(0.9, 0.2, 0.1, 1.0),
        },
        ColorStop {
            position: 0.5,
            color: Rgba::new(0.2, 0.7, 0.3, 1.0),
        },
        ColorStop {
            position: 1.0,
            color: Rgba::new(0.1, 0.2, 0.9, 1.0),
        },
    ]
}

#[test]
fn angular_and_diamond_gradients_match() {
    let (mut doc, page) = page();
    let a = add(
        &mut doc,
        page,
        2,
        NodeKind::Rectangle,
        Rect::new(20.0, 20.0, 80.0, 80.0),
    );
    doc.get_mut(a).unwrap().fills = vec![Paint {
        kind: deesainer_doc::PaintKind::AngularGradient {
            center: Point::new(0.4, 0.6),
            angle: 30.0,
            stops: ramp(),
        },
        ..solid(1.0, 1.0, 1.0, 1.0)
    }];
    let d = add(
        &mut doc,
        page,
        3,
        NodeKind::ellipse(),
        Rect::new(110.0, 20.0, 70.0, 80.0),
    );
    doc.get_mut(d).unwrap().fills = vec![Paint {
        kind: deesainer_doc::PaintKind::DiamondGradient {
            center: Point::new(0.5, 0.5),
            radius: Point::new(0.5, 0.5),
            stops: ramp(),
        },
        ..solid(1.0, 1.0, 1.0, 1.0)
    }];
    check("angular and diamond gradients", &doc, page);
}

#[test]
fn radial_and_multi_stop_gradients_match() {
    let (mut doc, page) = page();
    let r = add(
        &mut doc,
        page,
        2,
        NodeKind::ellipse(),
        Rect::new(20.0, 20.0, 90.0, 70.0),
    );
    doc.get_mut(r).unwrap().fills = vec![Paint {
        kind: deesainer_doc::PaintKind::RadialGradient {
            center: Point::new(0.4, 0.5),
            radius: Point::new(0.6, 0.5),
            stops: ramp(),
        },
        ..solid(1.0, 1.0, 1.0, 1.0)
    }];
    let l = add(
        &mut doc,
        page,
        3,
        NodeKind::Rectangle,
        Rect::new(120.0, 20.0, 60.0, 80.0),
    );
    doc.get_mut(l).unwrap().fills = vec![Paint::linear(
        Point::new(0.0, 0.0),
        Point::new(1.0, 1.0),
        ramp(),
    )];
    check("radial and multi-stop gradients", &doc, page);
}

#[test]
fn per_side_strokes_match() {
    let (mut doc, page) = page();
    for (i, (align, sides)) in [
        (deesainer_doc::StrokeAlign::Inside, [0.0, 4.0, 8.0, 2.0]),
        (deesainer_doc::StrokeAlign::Outside, [6.0, 0.0, 2.0, 4.0]),
    ]
    .into_iter()
    .enumerate()
    {
        let r = add(
            &mut doc,
            page,
            2 + i as u64,
            NodeKind::Rectangle,
            Rect::new(30.0 + i as f64 * 80.0, 30.0, 60.0, 50.0),
        );
        let n = doc.get_mut(r).unwrap();
        n.fills = vec![solid(0.9, 0.9, 0.2, 1.0)];
        n.strokes = vec![solid(0.1, 0.2, 0.8, 1.0)];
        n.corner_radius = deesainer_doc::CornerRadius::uniform(10.0);
        n.stroke.align = align;
        n.stroke.sides = Some(sides);
        n.stroke.weight = 8.0;
    }
    check("per-side strokes", &doc, page);
}
