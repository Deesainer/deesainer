use std::time::Instant;

use deesainer_engine::Session;
use serde_json::{json, Value};

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn median(runs: usize, mut f: impl FnMut()) -> f64 {
    let mut t: Vec<f64> = (0..runs)
        .map(|_| {
            let s = Instant::now();
            f();
            ms(s)
        })
        .collect();
    t.sort_by(f64::total_cmp);
    t[t.len() / 2]
}

fn big_document(n: usize) -> Result<Session, String> {
    let mut s = Session::new().map_err(|e| e.to_string())?;
    let frames = 100;
    let per = (n / frames).max(1);
    s.begin_group("bench");
    for f in 0..frames {
        let (fx, fy) = ((f % 10) as f64 * 1200.0, (f / 10) as f64 * 900.0);
        let frame = s
            .dispatch(
                "node.create.frame",
                &json!({"x": fx, "y": fy, "width": 1100, "height": 800}),
            )
            .map_err(|e| e.to_string())?["id"]
            .clone();
        for i in 0..per {
            let (x, y) = ((i % 20) as f64 * 54.0 + 10.0, (i / 20) as f64 * 40.0 + 10.0);
            let cmd = if i % 7 == 0 {
                "node.create.ellipse"
            } else {
                "node.create.rectangle"
            };
            s.dispatch(cmd, &json!({"parent": frame, "x": x, "y": y, "width": 44, "height": 30, "fill": "#4F7CFF"}))
                .map_err(|e| e.to_string())?;
        }
    }
    s.end_group();
    Ok(s)
}

fn gpu_scenarios(
    doc: &deesainer_engine::doc::Document,
    page: deesainer_engine::doc::NodeId,
    everything: &deesainer_mesh::SceneMesh,
    push: &mut impl FnMut(&str, f64, f64),
) {
    use std::sync::Arc;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("bench: no GPU adapter; GPU scenarios skipped");
        return;
    };
    let Ok((device, queue)) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
    else {
        eprintln!("bench: no GPU device; GPU scenarios skipped");
        return;
    };
    let mut compositor =
        deesainer_gpu::Compositor::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, 1, false);
    let frame = |mesh: Arc<deesainer_mesh::SceneMesh>, id: u64, dx: f32| deesainer_gpu::Frame {
        background: mesh.background.unwrap_or([0; 4]),
        mesh,
        mesh_id: id,
        textures: Arc::default(),
        view: deesainer_gpu::View {
            scale: 0.15,
            offset: [12.0 + dx, -7.0],
            size: [1920, 1080],
        },
    };
    let all = Arc::new(everything.clone());
    let start = Instant::now();
    if compositor
        .draw_and_wait(&device, &queue, &frame(Arc::clone(&all), 1, 0.0))
        .is_err()
    {
        eprintln!("bench: the GPU could not draw; GPU scenarios skipped");
        return;
    }
    push(
        "GPU: first frame, uploading all 10k layers (1920x1080)",
        ms(start),
        100.0,
    );
    let mut dx = 0.0;
    let t = median(9, || {
        dx += 3.0;
        let _ = compositor.draw_and_wait(&device, &queue, &frame(Arc::clone(&all), 1, dx));
    });
    push("GPU: a pan frame, all 10k layers (1920x1080)", t, 16.0);
    let mut faded = doc.clone();
    let frames: Vec<deesainer_engine::doc::NodeId> = faded
        .get(page)
        .map(|p| p.children.clone())
        .unwrap_or_default();
    for f in frames {
        if let Some(n) = faded.get_mut(f) {
            n.opacity = 0.8;
        }
    }
    let Ok(layered) = deesainer_mesh::build(
        &faded,
        page,
        deesainer_geom::Rect::new(-1e6, -1e6, 2e6, 2e6),
        0.15,
        Some(deesainer_engine::text_painter()),
    ) else {
        return;
    };
    let layered = Arc::new(layered);
    let _ = compositor.draw_and_wait(&device, &queue, &frame(Arc::clone(&layered), 2, 0.0));
    let t = median(9, || {
        dx += 3.0;
        let _ = compositor.draw_and_wait(&device, &queue, &frame(Arc::clone(&layered), 2, dx));
    });
    push(
        "GPU: a pan frame with 100 layers at 80% opacity (1920x1080)",
        t,
        16.0,
    );
    let mut shadowed = doc.clone();
    let frames: Vec<deesainer_engine::doc::NodeId> = shadowed
        .get(page)
        .map(|p| p.children.clone())
        .unwrap_or_default();
    for f in frames {
        if let Some(n) = shadowed.get_mut(f) {
            n.effects = vec![deesainer_engine::doc::Effect::DropShadow {
                color: deesainer_color::Rgba::new(0.0, 0.0, 0.0, 0.25),
                offset: deesainer_geom::Point::new(0.0, 8.0),
                blur: 24.0,
                spread: 0.0,
                visible: true,
            }];
        }
    }
    let Ok(shadows) = deesainer_mesh::build(
        &shadowed,
        page,
        deesainer_geom::Rect::new(-1e6, -1e6, 2e6, 2e6),
        0.15,
        Some(deesainer_engine::text_painter()),
    ) else {
        return;
    };
    let shadows = Arc::new(shadows);
    let _ = compositor.draw_and_wait(&device, &queue, &frame(Arc::clone(&shadows), 3, 0.0));
    let t = median(9, || {
        dx += 3.0;
        let _ = compositor.draw_and_wait(&device, &queue, &frame(Arc::clone(&shadows), 3, dx));
    });
    push(
        "GPU: a pan frame with 100 drop shadows (1920x1080)",
        t,
        16.0,
    );
}

pub fn run() -> Result<Value, String> {
    let mut scenarios = Vec::new();
    let mut push = |name: &str, ms: f64, budget: f64| {
        scenarios.push(json!({ "name": name, "ms": ms, "budgetMs": budget }));
    };

    let start = Instant::now();
    let session = Session::new().map_err(|e| e.to_string())?;
    push("Engine startup (registry, empty document)", ms(start), 50.0);
    let commands = session.registry().len();

    let mut small = Session::new().map_err(|e| e.to_string())?;
    let t = median(200, || {
        let _ = small.dispatch(
            "node.create.rectangle",
            &json!({"x": 0, "y": 0, "width": 10, "height": 10}),
        );
    });
    push(
        "Command dispatch (create rectangle, small document)",
        t,
        2.0,
    );

    let start = Instant::now();
    let mut big = big_document(10_000)?;
    push(
        "Build a 10k-layer document through commands",
        ms(start),
        20_000.0,
    );
    let t = median(9, || {
        let _ = big.dispatch("node.move", &json!({"ids": [5], "dx": 1, "dy": 0}));
    });
    push(
        "Command dispatch on a 10k-layer document (with derived passes)",
        t,
        50.0,
    );

    let doc = big.snapshot();
    let page = doc.first_page();
    let opts = deesainer_render::Options {
        background: true,
        text: Some(deesainer_engine::text_painter()),
        hidden: &[],
    };
    let viewport = |zoom: f64| deesainer_render::Viewport {
        transform: deesainer_geom::Affine::scale(zoom, zoom),
        width: 1920,
        height: 1080,
    };
    let t = median(5, || {
        let _ = deesainer_render::render_page(&doc, page, &viewport(1.0), opts);
    });
    push("Render 1920x1080 at 100% (about 2 frames visible)", t, 33.0);
    let t = median(3, || {
        let _ = deesainer_render::render_page(&doc, page, &viewport(0.15), opts);
    });
    push(
        "Render 1920x1080 zoomed out (all 10k layers visible)",
        t,
        250.0,
    );

    let region = |zoom: f64| deesainer_geom::Rect::new(0.0, 0.0, 1920.0 / zoom, 1080.0 / zoom);
    let text = Some(deesainer_engine::text_painter());
    let t = median(5, || {
        let _ = deesainer_mesh::build(&doc, page, region(1.0), 1.0, text);
    });
    push("Mesh: build the visible region at 100%", t, 50.0);
    let t = median(3, || {
        let _ = deesainer_mesh::build(
            &doc,
            page,
            deesainer_geom::Rect::new(-1e6, -1e6, 2e6, 2e6),
            0.15,
            text,
        );
    });
    push("Mesh: build all 10k layers zoomed out", t, 250.0);
    let everything = deesainer_mesh::build(
        &doc,
        page,
        deesainer_geom::Rect::new(-1e6, -1e6, 2e6, 2e6),
        0.15,
        text,
    )
    .map_err(|e| format!("the benchmark page must mesh: {e}"))?;
    let t = median(5, || {
        let mut out: Vec<([f32; 2], [u8; 4])> =
            Vec::with_capacity(everything.batches.iter().map(|b| b.vertices.len()).sum());
        for batch in &everything.batches {
            for v in &batch.vertices {
                out.push(([v.pos[0] * 0.15 + 12.0, v.pos[1] * 0.15 - 7.0], v.color));
            }
        }
        std::hint::black_box(&out);
    });
    push(
        "Mesh: reposition every vertex for a pan frame (10k layers)",
        t,
        16.0,
    );

    gpu_scenarios(&doc, page, &everything, &mut push);

    let t = median(3, || {
        for i in 0..1000 {
            let p = deesainer_geom::Point::new((i * 37 % 12000) as f64, (i * 53 % 9000) as f64);
            let _ = deesainer_vector::hit_test(&doc, page, p, 1.0, false);
        }
    }) / 1000.0;
    push("Hit-test one point (10k layers)", t, 1.0);

    let frame = doc.get(page).map(|p| p.children[0]).unwrap_or(page);
    let t = median(3, || {
        let _ = deesainer_io::export_png(&doc, frame, 3.5);
    });
    push("Export a 1100x800 frame at 4K width (3.5x PNG)", t, 2000.0);

    let start = Instant::now();
    let bytes = deesainer_format::write(&doc, None).map_err(|e| e.to_string())?;
    let save = ms(start);
    push("Save the 10k-layer document", save, 500.0);
    let start = Instant::now();
    let _ = deesainer_format::read(&bytes).map_err(|e| e.to_string())?;
    push("Open the 10k-layer document", ms(start), 500.0);

    let mut layout = Session::new().map_err(|e| e.to_string())?;
    let col = layout
        .dispatch(
            "node.create.frame",
            &json!({"x": 0, "y": 0, "width": 300, "height": 100}),
        )
        .map_err(|e| e.to_string())?["id"]
        .clone();
    for _ in 0..500 {
        let _ = layout.dispatch(
            "node.create.rectangle",
            &json!({"parent": col, "x": 0, "y": 0, "width": 280, "height": 20}),
        );
    }
    let _ = layout.dispatch(
        "layout.setAutoLayout",
        &json!({"id": col, "direction": "vertical", "gap": 4, "primarySizing": "hug"}),
    );
    let t = median(5, || {
        let _ = layout.dispatch("layout.setAutoLayout", &json!({"id": col, "gap": 5}));
        let _ = layout.dispatch("layout.setAutoLayout", &json!({"id": col, "gap": 4}));
    }) / 2.0;
    push("Auto layout re-solve (500 children)", t, 50.0);

    let machine = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    Ok(json!({
        "date": deesainer_automation::now_rfc3339(),
        "machine": machine,
        "commands": commands,
        "fileKb": bytes.len() / 1024,
        "scenarios": scenarios,
    }))
}
