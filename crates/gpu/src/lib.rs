use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use deesainer_color::BlendMode;
use deesainer_mesh::{Item, Pixels, SceneMesh, TextureRef};

pub const SAMPLES: u32 = 4;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const UNIFORM_STRIDE: u64 = 256;
const VERTEX_BYTES: u64 = 20;
const POOL_FRAMES: u64 = 120;
const POOL_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ATLAS_MEMBERS: usize = 256;
const CHILD_BYTES: u64 = 128 * 1024 * 1024;
const POOL_STEP: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub scale: f32,
    pub offset: [f32; 2],
    pub size: [u32; 2],
}

pub type TexturePixels = Arc<BTreeMap<TextureRef, Pixels>>;

#[derive(Clone)]
pub struct Frame {
    pub mesh: Arc<SceneMesh>,
    pub mesh_id: u64,
    pub textures: TexturePixels,
    pub view: View,
    pub background: [u8; 4],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuError(pub String);

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Default)]
pub struct Health(Arc<HealthState>);

#[derive(Default)]
struct HealthState {
    failure: std::sync::Mutex<Option<String>>,
    flattened: std::sync::atomic::AtomicUsize,
}

impl Health {
    pub fn failure(&self) -> Option<String> {
        self.0.failure.lock().ok().and_then(|g| g.clone())
    }

    pub fn flattened_layers(&self) -> usize {
        self.0.flattened.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn record(&self, message: String) {
        if let Ok(mut g) = self.0.failure.lock() {
            if g.is_none() {
                *g = Some(message);
            }
        }
    }
}

struct Target {
    msaa: wgpu::TextureView,
    resolve: wgpu::Texture,
    resolve_view: wgpu::TextureView,
    size: [u32; 2],
    last_used: u64,
    busy: bool,
}

#[derive(Clone, Copy)]
struct Placed {
    slot: usize,
    origin: [i32; 2],
    rect: [i32; 4],
    single: bool,
}

struct GpuTexture {
    bind: wgpu::BindGroup,
}

struct Uploaded {
    id: u64,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    ranges: Vec<(u32, u32, i32)>,
}

pub struct Compositor {
    mesh_pipeline: wgpu::RenderPipeline,
    over_pipeline: wgpu::RenderPipeline,
    blend_pipeline: wgpu::RenderPipeline,
    mask_pipeline: wgpu::RenderPipeline,
    mask_out_pipeline: wgpu::RenderPipeline,
    mesh_pipeline_1: wgpu::RenderPipeline,
    over_pipeline_1: wgpu::RenderPipeline,
    blend_pipeline_1: wgpu::RenderPipeline,
    tint_pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    transform_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    blit_layout: wgpu::BindGroupLayout,
    clamp: wgpu::Sampler,
    repeat: wgpu::Sampler,
    nearest: wgpu::Sampler,
    white: GpuTexture,
    textures: BTreeMap<TextureRef, GpuTexture>,
    uploaded: Option<Uploaded>,
    pool: Vec<Target>,
    frame: u64,
    uniforms: Option<wgpu::Buffer>,
    canvas: Option<Placed>,
    blit_bind: Option<wgpu::BindGroup>,
    blit_uniform: wgpu::Buffer,
    srgb_target: bool,
    target_samples: u32,
    flattened: usize,
    budget: u64,
    max_side: u32,
    health: Health,
}

fn blend_state(over: bool) -> Option<wgpu::BlendState> {
    over.then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)
}

const MASK_OUT_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

const MASK_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::SrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::SrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

fn mode_index(mode: BlendMode) -> u32 {
    match mode {
        BlendMode::PassThrough | BlendMode::Normal => 0,
        BlendMode::Darken => 1,
        BlendMode::Multiply => 2,
        BlendMode::ColorBurn => 3,
        BlendMode::Lighten => 4,
        BlendMode::Screen => 5,
        BlendMode::ColorDodge => 6,
        BlendMode::Overlay => 7,
        BlendMode::SoftLight => 8,
        BlendMode::HardLight => 9,
        BlendMode::Difference => 10,
        BlendMode::Exclusion => 11,
        BlendMode::Hue => 12,
        BlendMode::Saturation => 13,
        BlendMode::Color => 14,
        BlendMode::Luminosity => 15,
    }
}

fn uniform_entry(binding: u32, dynamic: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture_entry(binding: u32, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn pipeline(
    device: &wgpu::Device,
    label: &str,
    module: &wgpu::ShaderModule,
    layouts: &[Option<&wgpu::BindGroupLayout>],
    vertex_buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
    fragment: &str,
    format: wgpu::TextureFormat,
    samples: u32,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: layouts,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: vertex_buffers,
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: samples,
            ..wgpu::MultisampleState::default()
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn color(c: [u8; 4]) -> wgpu::Color {
    let f = |v: u8| f64::from(v) / 255.0;
    wgpu::Color {
        r: f(c[0]),
        g: f(c[1]),
        b: f(c[2]),
        a: f(c[3]),
    }
}

fn intersect(a: [i32; 4], b: [i32; 4]) -> Option<[i32; 4]> {
    let r = [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ];
    (r[0] < r[2] && r[1] < r[3]).then_some(r)
}

impl Compositor {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        target_samples: u32,
    ) -> Self {
        let health = Health::default();
        let sink = health.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            eprintln!("deesainer: GPU error: {e}");
            sink.record(e.to_string());
        }));
        let mesh_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("deesainer mesh"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mesh.wgsl").into()),
        });
        let composite_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("deesainer composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()),
        });
        let blit_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("deesainer blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        let transform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("deesainer transform"),
            entries: &[uniform_entry(0, true)],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("deesainer texture"),
            entries: &[texture_entry(0, true), sampler_entry(1)],
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("deesainer composite"),
            entries: &[
                uniform_entry(0, true),
                texture_entry(1, false),
                texture_entry(2, false),
            ],
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("deesainer blit"),
            entries: &[
                uniform_entry(0, false),
                texture_entry(1, true),
                sampler_entry(2),
            ],
        });
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: VERTEX_BYTES,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 8,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Unorm8x4,
                    offset: 16,
                    shader_location: 2,
                },
            ],
        };
        let mesh_pipeline = pipeline(
            device,
            "deesainer mesh",
            &mesh_module,
            &[Some(&transform_layout), Some(&texture_layout)],
            &[Some(vertex_layout.clone())],
            "fs_main",
            FORMAT,
            SAMPLES,
            blend_state(true),
        );
        let over_pipeline = pipeline(
            device,
            "deesainer composite over",
            &composite_module,
            &[Some(&composite_layout)],
            &[],
            "fs_over",
            FORMAT,
            SAMPLES,
            blend_state(true),
        );
        let blend_pipeline = pipeline(
            device,
            "deesainer composite blend",
            &composite_module,
            &[Some(&composite_layout)],
            &[],
            "fs_blend",
            FORMAT,
            SAMPLES,
            blend_state(false),
        );
        let mesh_pipeline_1 = pipeline(
            device,
            "deesainer mesh (1 sample)",
            &mesh_module,
            &[Some(&transform_layout), Some(&texture_layout)],
            &[Some(vertex_layout.clone())],
            "fs_main",
            FORMAT,
            1,
            blend_state(true),
        );
        let over_pipeline_1 = pipeline(
            device,
            "deesainer composite over (1 sample)",
            &composite_module,
            &[Some(&composite_layout)],
            &[],
            "fs_over",
            FORMAT,
            1,
            blend_state(true),
        );
        let blend_pipeline_1 = pipeline(
            device,
            "deesainer composite blend (1 sample)",
            &composite_module,
            &[Some(&composite_layout)],
            &[],
            "fs_blend",
            FORMAT,
            1,
            blend_state(false),
        );
        let blit_pipeline = pipeline(
            device,
            "deesainer blit",
            &blit_module,
            &[Some(&blit_layout)],
            &[],
            "fs_main",
            target_format,
            target_samples.max(1),
            blend_state(true),
        );
        let mask_pipeline = pipeline(
            device,
            "deesainer mask",
            &composite_module,
            &[Some(&composite_layout)],
            &[],
            "fs_over",
            FORMAT,
            1,
            Some(MASK_BLEND),
        );
        let mask_out_pipeline = pipeline(
            device,
            "deesainer mask outside",
            &composite_module,
            &[Some(&composite_layout)],
            &[],
            "fs_over",
            FORMAT,
            1,
            Some(MASK_OUT_BLEND),
        );
        let filter_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("deesainer filter"),
            source: wgpu::ShaderSource::Wgsl(include_str!("filter.wgsl").into()),
        });
        let tint_pipeline = pipeline(
            device,
            "deesainer tint",
            &filter_module,
            &[Some(&composite_layout)],
            &[],
            "fs_tint",
            FORMAT,
            1,
            None,
        );
        let blur_pipeline = pipeline(
            device,
            "deesainer blur",
            &filter_module,
            &[Some(&composite_layout)],
            &[],
            "fs_blur",
            FORMAT,
            1,
            None,
        );
        let sampler = |label: &str, mode: wgpu::AddressMode, filter: wgpu::FilterMode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: mode,
                address_mode_v: mode,
                mag_filter: filter,
                min_filter: filter,
                ..wgpu::SamplerDescriptor::default()
            })
        };
        let clamp = sampler(
            "deesainer clamp",
            wgpu::AddressMode::ClampToEdge,
            wgpu::FilterMode::Linear,
        );
        let repeat = sampler(
            "deesainer repeat",
            wgpu::AddressMode::Repeat,
            wgpu::FilterMode::Linear,
        );
        let nearest = sampler(
            "deesainer nearest",
            wgpu::AddressMode::ClampToEdge,
            wgpu::FilterMode::Nearest,
        );
        let white_pixels = Pixels {
            size: [1, 1],
            rgba: vec![255; 4],
        };
        let white = upload_texture(device, queue, &texture_layout, &clamp, &white_pixels);
        let blit_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("deesainer blit"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let srgb_target = target_format.is_srgb();
        Self {
            mesh_pipeline,
            over_pipeline,
            blend_pipeline,
            mask_pipeline,
            mask_out_pipeline,
            mesh_pipeline_1,
            over_pipeline_1,
            blend_pipeline_1,
            tint_pipeline,
            blur_pipeline,
            blit_pipeline,
            transform_layout,
            texture_layout,
            composite_layout,
            blit_layout,
            clamp,
            repeat,
            nearest,
            white,
            textures: BTreeMap::new(),
            uploaded: None,
            pool: Vec::new(),
            frame: 0,
            uniforms: None,
            canvas: None,
            blit_bind: None,
            blit_uniform,
            srgb_target,
            target_samples: target_samples.max(1),
            flattened: 0,
            budget: POOL_BYTES,
            max_side: device.limits().max_texture_dimension_2d,
            health,
        }
    }

    pub fn set_memory_budget(&mut self, bytes: u64) {
        self.budget = bytes;
    }

    pub fn health(&self) -> Health {
        self.health.clone()
    }

    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    fn sync_textures(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        let wanted: &BTreeSet<TextureRef> = &frame.mesh.textures;
        self.textures.retain(|t, _| wanted.contains(t));
        for t in wanted {
            if self.textures.contains_key(t) {
                continue;
            }
            let Some(pixels) = frame.textures.get(t) else {
                continue;
            };
            let [w, h] = pixels.size;
            let fits = w > 0
                && h > 0
                && w as u64 <= u64::from(self.max_side)
                && h as u64 <= u64::from(self.max_side)
                && pixels.rgba.len() as u64 == w as u64 * h as u64 * 4;
            if !fits {
                continue;
            }
            let sampler = match t {
                TextureRef::Image { repeat: true, .. } => &self.repeat,
                _ => &self.clamp,
            };
            let gpu = upload_texture(device, queue, &self.texture_layout, sampler, pixels);
            self.textures.insert(t.clone(), gpu);
        }
    }

    fn sync_mesh(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        if self
            .uploaded
            .as_ref()
            .is_some_and(|u| u.id == frame.mesh_id)
        {
            return;
        }
        let mesh = &frame.mesh;
        let vertex_count: usize = mesh.batches.iter().map(|b| b.vertices.len()).sum();
        let index_count: usize = mesh.batches.iter().map(|b| b.indices.len()).sum();
        let mut vertices = Vec::with_capacity(vertex_count * VERTEX_BYTES as usize);
        let mut indices = Vec::with_capacity(index_count * 4);
        let mut ranges = Vec::with_capacity(mesh.batches.len());
        let (mut first_vertex, mut first_index) = (0usize, 0usize);
        for b in &mesh.batches {
            for v in &b.vertices {
                for f in [v.pos[0], v.pos[1], v.uv[0], v.uv[1]] {
                    vertices.extend_from_slice(&f.to_le_bytes());
                }
                vertices.extend_from_slice(&v.color);
            }
            for i in &b.indices {
                indices.extend_from_slice(&i.to_le_bytes());
            }
            ranges.push((
                u32::try_from(first_index).unwrap_or(u32::MAX),
                u32::try_from(b.indices.len()).unwrap_or(0),
                i32::try_from(first_vertex).unwrap_or(i32::MAX),
            ));
            first_vertex += b.vertices.len();
            first_index += b.indices.len();
        }
        let pad = |v: &mut Vec<u8>| {
            while v.len() < 4 || !v.len().is_multiple_of(4) {
                v.push(0);
            }
        };
        pad(&mut vertices);
        pad(&mut indices);
        let buffer = |label: &str, data: &[u8], usage: wgpu::BufferUsages| {
            let b = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: data.len() as u64,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&b, 0, data);
            b
        };
        self.uploaded = Some(Uploaded {
            id: frame.mesh_id,
            vertices: buffer("deesainer vertices", &vertices, wgpu::BufferUsages::VERTEX),
            indices: buffer("deesainer indices", &indices, wgpu::BufferUsages::INDEX),
            ranges,
        });
    }

    fn take_target(&mut self, device: &wgpu::Device, size: [u32; 2]) -> usize {
        let free = self
            .pool
            .iter()
            .position(|t| !t.busy && t.size[0] >= size[0] && t.size[1] >= size[1]);
        if let Some(i) = free {
            if let Some(t) = self.pool.get_mut(i) {
                t.last_used = self.frame;
                t.busy = true;
            }
            return i;
        }
        let round = |v: u32| v.div_ceil(POOL_STEP).max(1).saturating_mul(POOL_STEP);
        let size = [
            round(size[0]).min(self.max_side),
            round(size[1]).min(self.max_side),
        ];
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let msaa = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("deesainer layer (msaa)"),
            size: extent,
            mip_level_count: 1,
            sample_count: SAMPLES,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let resolve = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("deesainer layer"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.pool.push(Target {
            msaa: msaa.create_view(&wgpu::TextureViewDescriptor::default()),
            resolve_view: resolve.create_view(&wgpu::TextureViewDescriptor::default()),
            resolve,
            size,
            last_used: self.frame,
            busy: true,
        });
        self.pool.len() - 1
    }

    fn busy_bytes(&self) -> u64 {
        self.pool
            .iter()
            .filter(|t| t.busy)
            .map(|t| u64::from(t.size[0]) * u64::from(t.size[1]) * 4 * u64::from(SAMPLES + 1))
            .sum()
    }

    fn release(&mut self, slot: usize) {
        if let Some(t) = self.pool.get_mut(slot) {
            t.busy = false;
        }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
    ) -> Result<(), GpuError> {
        if let Some(e) = self.health.failure() {
            return Err(GpuError(format!("the GPU reported an error: {e}")));
        }
        let [w, h] = frame.view.size;
        if w == 0 || h == 0 || w > self.max_side || h > self.max_side {
            return Err(GpuError(format!("a canvas of {w}x{h} pixels")));
        }
        self.frame += 1;
        self.flattened = 0;
        let now = self.frame;
        self.pool
            .retain(|t| now.saturating_sub(t.last_used) < POOL_FRAMES);
        for t in &mut self.pool {
            t.busy = false;
        }
        self.sync_textures(device, queue, frame);
        self.sync_mesh(device, queue, frame);
        let mut recorder = Recorder {
            uniforms: Vec::new(),
        };
        let mut plan = Vec::new();
        let root_slot = self.take_target(device, [w, h]);
        let root = Placed {
            slot: root_slot,
            origin: [0, 0],
            rect: [0, 0, w as i32, h as i32],
            single: false,
        };
        let mut index = 0;
        self.plan_list(
            device,
            &frame.mesh,
            frame.view,
            root,
            Some(frame.background),
            &mut index,
            &mut plan,
            &mut recorder,
        );
        let needed = (recorder.uniforms.len() as u64).max(UNIFORM_STRIDE);
        let reuse = self.uniforms.as_ref().is_some_and(|b| b.size() >= needed);
        if !reuse {
            self.uniforms = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("deesainer uniforms"),
                size: needed.next_power_of_two(),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let Some(uniforms) = self.uniforms.as_ref() else {
            return Err(GpuError("no uniform buffer".into()));
        };
        if !recorder.uniforms.is_empty() {
            queue.write_buffer(uniforms, 0, &recorder.uniforms);
        }
        self.record(device, encoder, &plan, uniforms);
        let Some(root_target) = self.pool.get(root_slot) else {
            return Err(GpuError("the canvas target is missing".into()));
        };
        let mut blit = Vec::with_capacity(16);
        blit.extend_from_slice(&u32::from(self.srgb_target).to_le_bytes());
        blit.extend_from_slice(&0u32.to_le_bytes());
        blit.extend_from_slice(&(w as f32 / root_target.size[0] as f32).to_le_bytes());
        blit.extend_from_slice(&(h as f32 / root_target.size[1] as f32).to_le_bytes());
        queue.write_buffer(&self.blit_uniform, 0, &blit);
        let view = root_target
            .resolve
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.blit_bind = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("deesainer blit"),
            layout: &self.blit_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.blit_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.nearest),
                },
            ],
        }));
        self.canvas = Some(root);
        self.health
            .0
            .flattened
            .store(self.flattened, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_list(
        &mut self,
        device: &wgpu::Device,
        mesh: &SceneMesh,
        view: View,
        target: Placed,
        clear: Option<[u8; 4]>,
        index: &mut usize,
        plan: &mut Vec<Step>,
        rec: &mut Recorder,
    ) {
        let size = self.pool.get(target.slot).map_or([1, 1], |t| t.size);
        let transform = rec.transform(view, target.origin, size);
        let mut ops: Vec<Op> = Vec::new();
        let mut waiting: Vec<usize> = Vec::new();
        let mut waiting_bytes = 0u64;
        let mut clear = clear;
        let mut inline = 0usize;
        let mut group: Vec<Member> = Vec::new();
        let mut group_bytes = 0u64;
        let mut flush = |this: &mut Self,
                         ops: &mut Vec<Op>,
                         waiting: &mut Vec<usize>,
                         plan: &mut Vec<Step>,
                         group: &mut Vec<Member>,
                         rec: &mut Recorder| {
            if !group.is_empty() {
                let placed = this.plan_atlases(device, mesh, view, group, plan, rec, waiting);
                for op in ops.iter_mut() {
                    if let Op::Pending(k) = *op {
                        let (Some(m), Some(&(source, at))) = (group.get(k), placed.get(k)) else {
                            continue;
                        };
                        let local = [
                            (m.rect[0] - target.origin[0]) as f32,
                            (m.rect[1] - target.origin[1]) as f32,
                            (m.rect[2] - target.origin[0]) as f32,
                            (m.rect[3] - target.origin[1]) as f32,
                        ];
                        let uniform = rec.composite(local, size, m.layer.opacity, 0, at);
                        *op = Op::Over { source, uniform };
                    }
                }
                group.clear();
            }
            if !ops.is_empty() || clear.is_some() {
                plan.push(Step::Pass {
                    slot: target.slot,
                    single: target.single,
                    clear: clear.take(),
                    transform,
                    ops: std::mem::take(ops),
                });
            }
            for slot in waiting.drain(..) {
                this.release(slot);
            }
        };
        while let Some(item) = mesh.items.get(*index) {
            *index += 1;
            match item {
                Item::Batch(b) => {
                    let Some(batch) = mesh.batches.get(*b) else {
                        continue;
                    };
                    let mut scissor = target.rect;
                    if let Some(c) = batch.clip {
                        let px = |x: f32, o: f32| x * view.scale + o;
                        let clip = [
                            px(c[0], view.offset[0]).floor() as i32,
                            px(c[1], view.offset[1]).floor() as i32,
                            px(c[2], view.offset[0]).ceil() as i32,
                            px(c[3], view.offset[1]).ceil() as i32,
                        ];
                        match intersect(scissor, clip) {
                            Some(r) => scissor = r,
                            None => continue,
                        }
                    }
                    ops.push(Op::Draw(Draw {
                        batch: *b,
                        texture: batch.texture.clone(),
                        transform: None,
                        scissor: [
                            scissor[0] - target.origin[0],
                            scissor[1] - target.origin[1],
                            scissor[2] - target.origin[0],
                            scissor[3] - target.origin[1],
                        ],
                    }));
                }
                Item::Begin(layer) => {
                    let rect = match layer.bounds {
                        None => Some(target.rect),
                        Some(b) => {
                            let px = |x: f32, o: f32| x * view.scale + o;
                            intersect(
                                target.rect,
                                [
                                    px(b[0], view.offset[0]).floor() as i32 - 1,
                                    px(b[1], view.offset[1]).floor() as i32 - 1,
                                    px(b[2], view.offset[0]).ceil() as i32 + 1,
                                    px(b[3], view.offset[1]).ceil() as i32 + 1,
                                ],
                            )
                        }
                    };
                    let Some(rect) = rect else {
                        skip_layer(mesh, index);
                        continue;
                    };
                    let need = [(rect[2] - rect[0]) as u32, (rect[3] - rect[1]) as u32];
                    let targets = 1
                        + u64::from(layer.masked)
                        + u64::from(layer.blur > 0.0 || layer.tint.is_some());
                    let bytes = u64::from(need[0].div_ceil(POOL_STEP) * POOL_STEP)
                        * u64::from(need[1].div_ceil(POOL_STEP) * POOL_STEP)
                        * 4
                        * u64::from(SAMPLES + 1)
                        * targets;
                    if self.busy_bytes() + bytes > self.budget {
                        self.flattened += 1;
                        if layer.masked {
                            skip_mask(mesh, index);
                        }
                        inline += 1;
                        continue;
                    }
                    if mode_index(layer.blend) == 0 && !layer.backdrop {
                        if let Some((mask_batches, content, next)) =
                            leaf_items(mesh, *index, layer.masked)
                        {
                            *index = next;
                            ops.push(Op::Pending(group.len()));
                            group_bytes += u64::from(need[0])
                                * u64::from(need[1])
                                * 4
                                * u64::from(SAMPLES + 1)
                                * if layer.masked { 2 } else { 1 };
                            group.push(Member {
                                layer: layer.clone(),
                                rect,
                                need,
                                mask: mask_batches,
                                content,
                            });
                            if group_bytes > CHILD_BYTES || group.len() >= MAX_ATLAS_MEMBERS {
                                flush(self, &mut ops, &mut waiting, plan, &mut group, rec);
                                group_bytes = 0;
                                waiting_bytes = 0;
                            }
                            continue;
                        }
                    }
                    let mask = if layer.masked {
                        let slot = self.take_target(device, need);
                        let twin = Placed {
                            slot,
                            origin: [rect[0], rect[1]],
                            rect,
                            single: false,
                        };
                        self.plan_list(device, mesh, view, twin, Some([0; 4]), index, plan, rec);
                        Some(slot)
                    } else {
                        None
                    };
                    let slot = self.take_target(device, need);
                    let child = Placed {
                        slot,
                        origin: [rect[0], rect[1]],
                        rect,
                        single: layer.blur / 2.0 * view.scale >= 1.0,
                    };
                    let child_size = self.pool.get(slot).map_or([1, 1], |t| t.size);
                    if layer.backdrop {
                        flush(self, &mut ops, &mut waiting, plan, &mut group, rec);
                        waiting_bytes = 0;
                        group_bytes = 0;
                        plan.push(Step::Copy {
                            from: target.slot,
                            to: slot,
                            origin: [
                                (rect[0] - target.origin[0]).max(0) as u32,
                                (rect[1] - target.origin[1]).max(0) as u32,
                            ],
                            size: need,
                        });
                    }
                    let start = if layer.backdrop { None } else { Some([0; 4]) };
                    self.plan_list(device, mesh, view, child, start, index, plan, rec);
                    let sigma = (layer.blur / 2.0 * view.scale).min(250.0);
                    let tinted = layer.tint.is_some() || layer.invert;
                    let blurred = layer.blur > 0.0 && sigma >= 0.25;
                    if tinted || blurred {
                        let tmp = self.take_target(device, need);
                        let tmp_size = self.pool.get(tmp).map_or([1, 1], |t| t.size);
                        let tint = layer.tint.unwrap_or([1.0; 4]);
                        let mut kinds = Vec::new();
                        if blurred {
                            kinds.push((FilterKind::Blur, sigma, 0));
                            kinds.push((FilterKind::Blur, sigma, 1));
                        } else {
                            kinds.push((FilterKind::Tint, 0.0, 0));
                        }
                        let passes: Vec<Vec<(FilterKind, u32)>> = kinds
                            .into_iter()
                            .enumerate()
                            .map(|(i, (kind, sigma, axis))| {
                                let dst_size = if i % 2 == 0 { tmp_size } else { child_size };
                                let first = i == 0;
                                let uniform = rec.filter(
                                    dst_size,
                                    need,
                                    tint,
                                    sigma,
                                    axis,
                                    first && layer.invert,
                                    first && layer.tint.is_some(),
                                    [0, 0],
                                );
                                vec![(kind, uniform)]
                            })
                            .collect();
                        plan.push(Step::Filter {
                            slot,
                            tmp,
                            size: need,
                            passes,
                        });
                        self.release(tmp);
                    }
                    if let Some(mask_slot) = mask {
                        let full = [0.0, 0.0, need[0] as f32, need[1] as f32];
                        let uniform = rec.composite(full, child_size, 1.0, 0, [0.0, 0.0]);
                        plan.push(Step::Mask {
                            slot,
                            mask: mask_slot,
                            uniforms: vec![(uniform, layer.mask_outside)],
                        });
                        self.release(mask_slot);
                    }
                    let local = [
                        (rect[0] - target.origin[0]) as f32,
                        (rect[1] - target.origin[1]) as f32,
                        (rect[2] - target.origin[0]) as f32,
                        (rect[3] - target.origin[1]) as f32,
                    ];
                    let mode = mode_index(layer.blend);
                    let uniform = rec.composite(local, size, layer.opacity, mode, [0.0, 0.0]);
                    if mode == 0 {
                        ops.push(Op::Over {
                            source: slot,
                            uniform,
                        });
                        waiting.push(slot);
                        waiting_bytes += self.pool.get(slot).map_or(0, |t| {
                            u64::from(t.size[0]) * u64::from(t.size[1]) * 4 * u64::from(SAMPLES + 1)
                        });
                        if waiting_bytes > CHILD_BYTES {
                            flush(self, &mut ops, &mut waiting, plan, &mut group, rec);
                            waiting_bytes = 0;
                            group_bytes = 0;
                        }
                    } else {
                        flush(self, &mut ops, &mut waiting, plan, &mut group, rec);
                        waiting_bytes = 0;
                        group_bytes = 0;
                        plan.push(Step::Blend {
                            source: slot,
                            target: target.slot,
                            single: target.single,
                            rect: local,
                            uniform,
                        });
                        self.release(slot);
                    }
                }
                Item::End if inline > 0 => inline -= 1,
                Item::MaskEnd | Item::End => break,
            }
        }
        flush(self, &mut ops, &mut waiting, plan, &mut group, rec);
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_atlases(
        &mut self,
        device: &wgpu::Device,
        mesh: &SceneMesh,
        view: View,
        members: &[Member],
        plan: &mut Vec<Step>,
        rec: &mut Recorder,
        waiting: &mut Vec<usize>,
    ) -> Vec<(usize, [f32; 2])> {
        const GAP: u32 = 2;
        let widest = members.iter().map(|m| m.need[0]).max().unwrap_or(1);
        let width = (widest + GAP).max(1024).min(self.max_side);
        let mut placed = Vec::with_capacity(members.len());
        let mut start = 0;
        while start < members.len() {
            let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
            let mut spots: Vec<[u32; 2]> = Vec::new();
            let mut end = start;
            while let Some(m) = members.get(end) {
                let [w, h] = m.need;
                if x > 0 && x + w > width {
                    x = 0;
                    y += shelf + GAP;
                    shelf = 0;
                }
                if y + h > self.max_side && end > start {
                    break;
                }
                spots.push([x, y]);
                x += w + GAP;
                shelf = shelf.max(h);
                end += 1;
            }
            let height = (y + shelf).clamp(1, self.max_side);
            let batch = members.get(start..end).unwrap_or_default();
            let ids = self.plan_atlas(
                device,
                mesh,
                view,
                batch,
                &spots,
                [width, height],
                plan,
                rec,
            );
            waiting.push(ids);
            for spot in &spots {
                placed.push((ids, [spot[0] as f32, spot[1] as f32]));
            }
            start = end.max(start + 1);
        }
        placed
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_atlas(
        &mut self,
        device: &wgpu::Device,
        mesh: &SceneMesh,
        view: View,
        members: &[Member],
        spots: &[[u32; 2]],
        size: [u32; 2],
        plan: &mut Vec<Step>,
        rec: &mut Recorder,
    ) -> usize {
        let sigma_of = |m: &Member| (m.layer.blur / 2.0 * view.scale).min(250.0);
        let single = members.iter().all(|m| sigma_of(m) >= 1.0);
        let atlas = self.take_target(device, size);
        let atlas_size = self.pool.get(atlas).map_or([1, 1], |t| t.size);
        let draws = |batches_of: &dyn Fn(&Member) -> &[usize], rec: &mut Recorder| {
            let mut ops = Vec::new();
            for (m, spot) in members.iter().zip(spots) {
                let shift = [m.rect[0] - spot[0] as i32, m.rect[1] - spot[1] as i32];
                let transform = rec.transform(view, shift, atlas_size);
                for b in batches_of(m) {
                    let Some(batch) = mesh.batches.get(*b) else {
                        continue;
                    };
                    let mut scissor = m.rect;
                    if let Some(c) = batch.clip {
                        let px = |x: f32, o: f32| x * view.scale + o;
                        let clip = [
                            px(c[0], view.offset[0]).floor() as i32,
                            px(c[1], view.offset[1]).floor() as i32,
                            px(c[2], view.offset[0]).ceil() as i32,
                            px(c[3], view.offset[1]).ceil() as i32,
                        ];
                        match intersect(scissor, clip) {
                            Some(r) => scissor = r,
                            None => continue,
                        }
                    }
                    ops.push(Op::Draw(Draw {
                        batch: *b,
                        texture: batch.texture.clone(),
                        transform: Some(transform),
                        scissor: [
                            scissor[0] - shift[0],
                            scissor[1] - shift[1],
                            scissor[2] - shift[0],
                            scissor[3] - shift[1],
                        ],
                    }));
                }
            }
            ops
        };
        let masked = members.iter().any(|m| m.layer.masked);
        let mask = if masked {
            let slot = self.take_target(device, size);
            let ops = draws(&|m: &Member| m.mask.as_slice(), rec);
            plan.push(Step::Pass {
                slot,
                single: false,
                clear: Some([0; 4]),
                transform: 0,
                ops,
            });
            Some(slot)
        } else {
            None
        };
        let ops = draws(&|m: &Member| m.content.as_slice(), rec);
        plan.push(Step::Pass {
            slot: atlas,
            single,
            clear: Some([0; 4]),
            transform: 0,
            ops,
        });
        let blurred = |m: &Member| m.layer.blur > 0.0 && sigma_of(m) >= 0.25;
        let filtered = |m: &Member| blurred(m) || m.layer.tint.is_some() || m.layer.invert;
        if members.iter().any(filtered) {
            let tmp = self.take_target(device, size);
            let tmp_size = self.pool.get(tmp).map_or([1, 1], |t| t.size);
            let count = if members.iter().any(blurred) { 2 } else { 1 };
            let passes = (0..count)
                .map(|p| {
                    let dst = if p % 2 == 0 { tmp_size } else { atlas_size };
                    members
                        .iter()
                        .zip(spots)
                        .map(|(m, spot)| {
                            let tint = m.layer.tint.unwrap_or([1.0; 4]);
                            let first = p == 0;
                            let (kind, sigma, axis) = if blurred(m) {
                                (FilterKind::Blur, sigma_of(m), p as u32)
                            } else {
                                (FilterKind::Tint, 0.0, 0)
                            };
                            let uniform = rec.filter(
                                dst,
                                m.need,
                                tint,
                                sigma,
                                axis,
                                first && m.layer.invert,
                                first && m.layer.tint.is_some(),
                                *spot,
                            );
                            (kind, uniform)
                        })
                        .collect()
                })
                .collect();
            plan.push(Step::Filter {
                slot: atlas,
                tmp,
                size,
                passes,
            });
            self.release(tmp);
        }
        if let Some(mask_slot) = mask {
            let uniforms = members
                .iter()
                .zip(spots)
                .filter(|(m, _)| m.layer.masked)
                .map(|(m, spot)| {
                    let (x, y) = (spot[0] as f32, spot[1] as f32);
                    let r = [x, y, x + m.need[0] as f32, y + m.need[1] as f32];
                    (
                        rec.composite(r, atlas_size, 1.0, 0, [x, y]),
                        m.layer.mask_outside,
                    )
                })
                .collect();
            plan.push(Step::Mask {
                slot: atlas,
                mask: mask_slot,
                uniforms,
            });
            self.release(mask_slot);
        }
        atlas
    }

    fn composite_bind(
        &self,
        device: &wgpu::Device,
        uniforms: &wgpu::Buffer,
        source: &wgpu::TextureView,
        backdrop: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("deesainer composite"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: uniforms,
                        offset: 0,
                        size: wgpu::BufferSize::new(64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(backdrop),
                },
            ],
        })
    }

    fn record(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        plan: &[Step],
        uniforms: &wgpu::Buffer,
    ) {
        let transform_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("deesainer transform"),
            layout: &self.transform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: uniforms,
                    offset: 0,
                    size: wgpu::BufferSize::new(32),
                }),
            }],
        });
        for step in plan {
            match step {
                Step::Pass {
                    slot,
                    single,
                    clear,
                    transform,
                    ops,
                } => {
                    let Some(t) = self.pool.get(*slot) else {
                        continue;
                    };
                    let binds: Vec<Option<wgpu::BindGroup>> = ops
                        .iter()
                        .map(|op| match op {
                            Op::Over { source, .. } => self.pool.get(*source).map(|src| {
                                self.composite_bind(
                                    device,
                                    uniforms,
                                    &src.resolve_view,
                                    &src.resolve_view,
                                )
                            }),
                            Op::Draw(_) | Op::Pending(_) => None,
                        })
                        .collect();
                    let load = match clear {
                        Some(c) => wgpu::LoadOp::Clear(color(*c)),
                        None => wgpu::LoadOp::Load,
                    };
                    let mut pass = if *single {
                        begin_single(encoder, &t.resolve_view, load)
                    } else {
                        begin(encoder, t, load)
                    };
                    let (mesh_pipeline, over_pipeline) = if *single {
                        (&self.mesh_pipeline_1, &self.over_pipeline_1)
                    } else {
                        (&self.mesh_pipeline, &self.over_pipeline)
                    };
                    let mut mesh_bound = false;
                    let mut bound_transform = u32::MAX;
                    for (op, bind) in ops.iter().zip(&binds) {
                        match op {
                            Op::Draw(d) => {
                                let Some(up) = self.uploaded.as_ref() else {
                                    continue;
                                };
                                let Some(&(first, count, base)) = up.ranges.get(d.batch) else {
                                    continue;
                                };
                                if count == 0 {
                                    continue;
                                }
                                let texture = match &d.texture {
                                    None => &self.white,
                                    Some(r) => match self.textures.get(r) {
                                        Some(g) => g,
                                        None => continue,
                                    },
                                };
                                let Some(sc) = scissor(d.scissor, t.size) else {
                                    continue;
                                };
                                let wanted = d.transform.unwrap_or(*transform);
                                if mesh_bound && bound_transform != wanted {
                                    pass.set_bind_group(0, &transform_bind, &[wanted]);
                                    bound_transform = wanted;
                                }
                                if !mesh_bound {
                                    pass.set_pipeline(mesh_pipeline);
                                    pass.set_bind_group(0, &transform_bind, &[wanted]);
                                    bound_transform = wanted;
                                    pass.set_vertex_buffer(0, up.vertices.slice(..));
                                    pass.set_index_buffer(
                                        up.indices.slice(..),
                                        wgpu::IndexFormat::Uint32,
                                    );
                                    mesh_bound = true;
                                }
                                pass.set_scissor_rect(sc[0], sc[1], sc[2], sc[3]);
                                pass.set_bind_group(1, &texture.bind, &[]);
                                pass.draw_indexed(first..first + count, base, 0..1);
                            }
                            Op::Pending(_) => {}
                            Op::Over { uniform, .. } => {
                                let Some(bind) = bind else { continue };
                                pass.set_scissor_rect(0, 0, t.size[0], t.size[1]);
                                pass.set_pipeline(over_pipeline);
                                pass.set_bind_group(0, bind, &[*uniform]);
                                pass.draw(0..6, 0..1);
                                mesh_bound = false;
                            }
                        }
                    }
                }
                Step::Copy {
                    from,
                    to,
                    origin,
                    size,
                } => {
                    let (Some(src), Some(dst)) = (self.pool.get(*from), self.pool.get(*to)) else {
                        continue;
                    };
                    let w = size[0]
                        .min(src.size[0].saturating_sub(origin[0]))
                        .min(dst.size[0]);
                    let h = size[1]
                        .min(src.size[1].saturating_sub(origin[1]))
                        .min(dst.size[1]);
                    if w == 0 || h == 0 {
                        continue;
                    }
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &src.resolve,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: origin[0],
                                y: origin[1],
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::TexelCopyTextureInfo {
                            texture: &dst.resolve,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::Extent3d {
                            width: w,
                            height: h,
                            depth_or_array_layers: 1,
                        },
                    );
                }
                Step::Filter {
                    slot,
                    tmp,
                    size,
                    passes,
                } => {
                    let (Some(layer), Some(spare)) = (self.pool.get(*slot), self.pool.get(*tmp))
                    else {
                        continue;
                    };
                    let mut in_layer = true;
                    for quads in passes {
                        let (src, dst) = if in_layer {
                            (layer, spare)
                        } else {
                            (spare, layer)
                        };
                        let bind = self.composite_bind(
                            device,
                            uniforms,
                            &src.resolve_view,
                            &src.resolve_view,
                        );
                        let mut pass = begin_single(
                            encoder,
                            &dst.resolve_view,
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        );
                        for (kind, uniform) in quads {
                            pass.set_pipeline(match kind {
                                FilterKind::Tint => &self.tint_pipeline,
                                FilterKind::Blur => &self.blur_pipeline,
                            });
                            pass.set_bind_group(0, &bind, &[*uniform]);
                            pass.draw(0..6, 0..1);
                        }
                        drop(pass);
                        in_layer = !in_layer;
                    }
                    if !in_layer {
                        let w = size[0].min(layer.size[0]).min(spare.size[0]);
                        let h = size[1].min(layer.size[1]).min(spare.size[1]);
                        encoder.copy_texture_to_texture(
                            wgpu::TexelCopyTextureInfo {
                                texture: &spare.resolve,
                                mip_level: 0,
                                origin: wgpu::Origin3d::ZERO,
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::TexelCopyTextureInfo {
                                texture: &layer.resolve,
                                mip_level: 0,
                                origin: wgpu::Origin3d::ZERO,
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::Extent3d {
                                width: w,
                                height: h,
                                depth_or_array_layers: 1,
                            },
                        );
                    }
                }
                Step::Mask {
                    slot,
                    mask,
                    uniforms: quads,
                } => {
                    let (Some(layer), Some(m)) = (self.pool.get(*slot), self.pool.get(*mask))
                    else {
                        continue;
                    };
                    let bind =
                        self.composite_bind(device, uniforms, &m.resolve_view, &m.resolve_view);
                    let mut pass = begin_single(encoder, &layer.resolve_view, wgpu::LoadOp::Load);
                    for (uniform, outside) in quads {
                        pass.set_pipeline(if *outside {
                            &self.mask_out_pipeline
                        } else {
                            &self.mask_pipeline
                        });
                        pass.set_bind_group(0, &bind, &[*uniform]);
                        pass.draw(0..6, 0..1);
                    }
                }
                Step::Blend {
                    source,
                    target,
                    single,
                    rect,
                    uniform,
                } => {
                    let (Some(src), Some(dst)) = (self.pool.get(*source), self.pool.get(*target))
                    else {
                        continue;
                    };
                    let w = (rect[2] - rect[0]).max(0.0) as u32;
                    let h = (rect[3] - rect[1]).max(0.0) as u32;
                    if w == 0 || h == 0 {
                        continue;
                    }
                    let extent = wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    };
                    let copy = device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("deesainer backdrop"),
                        size: extent,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: FORMAT,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    });
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &dst.resolve,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: rect[0] as u32,
                                y: rect[1] as u32,
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::TexelCopyTextureInfo {
                            texture: &copy,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        extent,
                    );
                    let backdrop = copy.create_view(&wgpu::TextureViewDescriptor::default());
                    let bind = self.composite_bind(device, uniforms, &src.resolve_view, &backdrop);
                    let mut pass = if *single {
                        begin_single(encoder, &dst.resolve_view, wgpu::LoadOp::Load)
                    } else {
                        begin(encoder, dst, wgpu::LoadOp::Load)
                    };
                    pass.set_pipeline(if *single {
                        &self.blend_pipeline_1
                    } else {
                        &self.blend_pipeline
                    });
                    pass.set_bind_group(0, &bind, &[*uniform]);
                    pass.draw(0..6, 0..1);
                }
            }
        }
    }

    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(bind) = &self.blit_bind else { return };
        pass.set_pipeline(&self.blit_pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..6, 0..1);
    }

    pub fn draw_and_wait(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
    ) -> Result<(), GpuError> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("deesainer timing"),
        });
        self.prepare(device, queue, &mut encoder, frame)?;
        queue.submit([encoder.finish()]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map(|_| ())
            .map_err(|e| GpuError(format!("waiting for the GPU: {e}")))
    }

    pub fn render_to_pixels(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
    ) -> Result<Vec<u8>, GpuError> {
        if self.srgb_target || self.target_samples != 1 {
            return Err(GpuError(
                "reading back needs a compositor made for Rgba8Unorm with one sample".into(),
            ));
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("deesainer readback"),
        });
        self.prepare(device, queue, &mut encoder, frame)?;
        let [w, h] = frame.view.size;
        let screen = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("deesainer readback screen"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let screen_view = screen.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("deesainer readback screen"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &screen_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.paint(&mut pass);
        }
        let target = &screen;
        let row = (w * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("deesainer readback"),
            size: u64::from(row) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| GpuError(format!("waiting for the GPU: {e}")))?;
        match rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(GpuError(format!("reading the canvas: {e}"))),
            Err(e) => return Err(GpuError(format!("reading the canvas: {e}"))),
        }
        let data = slice
            .get_mapped_range()
            .map_err(|e| GpuError(format!("reading the canvas: {e}")))?;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize {
            let start = y * row as usize;
            if let Some(line) = data.get(start..start + (w * 4) as usize) {
                out.extend_from_slice(line);
            }
        }
        drop(data);
        buffer.unmap();
        Ok(out)
    }
}

fn begin_single<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("deesainer filter"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn scissor(s: [i32; 4], size: [u32; 2]) -> Option<[u32; 4]> {
    let (x0, y0) = (s[0].max(0) as u32, s[1].max(0) as u32);
    let (x1, y1) = (
        (s[2].max(0) as u32).min(size[0]),
        (s[3].max(0) as u32).min(size[1]),
    );
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0])
}

fn skip_mask(mesh: &SceneMesh, index: &mut usize) {
    let mut depth = 0usize;
    while let Some(item) = mesh.items.get(*index) {
        *index += 1;
        match item {
            Item::Begin(_) => depth += 1,
            Item::End => depth = depth.saturating_sub(1),
            Item::MaskEnd if depth == 0 => return,
            _ => {}
        }
    }
}

struct Member {
    layer: deesainer_mesh::Layer,
    rect: [i32; 4],
    need: [u32; 2],
    mask: Vec<usize>,
    content: Vec<usize>,
}

fn leaf_items(
    mesh: &SceneMesh,
    index: usize,
    masked: bool,
) -> Option<(Vec<usize>, Vec<usize>, usize)> {
    let (mut mask, mut content) = (Vec::new(), Vec::new());
    let mut in_mask = masked;
    let mut i = index;
    while let Some(item) = mesh.items.get(i) {
        i += 1;
        match item {
            Item::Batch(b) if in_mask => mask.push(*b),
            Item::Batch(b) => content.push(*b),
            Item::MaskEnd => in_mask = false,
            Item::End => return Some((mask, content, i)),
            Item::Begin(_) => return None,
        }
    }
    None
}

fn skip_layer(mesh: &SceneMesh, index: &mut usize) {
    let mut depth = 1usize;
    while let Some(item) = mesh.items.get(*index) {
        *index += 1;
        match item {
            Item::Begin(_) => depth += 1,
            Item::End => {
                depth -= 1;
                if depth == 0 {
                    return;
                }
            }
            Item::Batch(_) | Item::MaskEnd => {}
        }
    }
}

fn begin<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    t: &Target,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("deesainer layer"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &t.msaa,
            depth_slice: None,
            resolve_target: Some(&t.resolve_view),
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

struct Draw {
    batch: usize,
    texture: Option<TextureRef>,
    transform: Option<u32>,
    scissor: [i32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FilterKind {
    Tint,
    Blur,
}

enum Op {
    Draw(Draw),
    Pending(usize),
    Over { source: usize, uniform: u32 },
}

enum Step {
    Pass {
        slot: usize,
        single: bool,
        clear: Option<[u8; 4]>,
        transform: u32,
        ops: Vec<Op>,
    },
    Copy {
        from: usize,
        to: usize,
        origin: [u32; 2],
        size: [u32; 2],
    },
    Filter {
        slot: usize,
        tmp: usize,
        size: [u32; 2],
        passes: Vec<Vec<(FilterKind, u32)>>,
    },
    Mask {
        slot: usize,
        mask: usize,
        uniforms: Vec<(u32, bool)>,
    },
    Blend {
        source: usize,
        target: usize,
        single: bool,
        rect: [f32; 4],
        uniform: u32,
    },
}

struct Recorder {
    uniforms: Vec<u8>,
}

impl Recorder {
    fn slot(&mut self, words: &[f32]) -> u32 {
        let at = self.uniforms.len();
        for w in words {
            self.uniforms.extend_from_slice(&w.to_le_bytes());
        }
        self.uniforms.resize(at + UNIFORM_STRIDE as usize, 0);
        u32::try_from(at).unwrap_or(0)
    }

    fn transform(&mut self, view: View, origin: [i32; 2], size: [u32; 2]) -> u32 {
        self.slot(&[
            view.scale,
            0.0,
            view.offset[0] - origin[0] as f32,
            view.offset[1] - origin[1] as f32,
            size[0] as f32,
            size[1] as f32,
            0.0,
            0.0,
        ])
    }

    #[allow(clippy::too_many_arguments)]
    fn filter(
        &mut self,
        size: [u32; 2],
        used: [u32; 2],
        tint: [f32; 4],
        sigma: f32,
        axis: u32,
        invert: bool,
        tinted: bool,
        origin: [u32; 2],
    ) -> u32 {
        let at = self.slot(&[
            size[0] as f32,
            size[1] as f32,
            used[0] as f32,
            used[1] as f32,
            tint[0],
            tint[1],
            tint[2],
            tint[3],
            sigma,
        ]);
        let i = at as usize + 36;
        for (k, v) in [axis, u32::from(invert), u32::from(tinted)]
            .into_iter()
            .enumerate()
        {
            if let Some(word) = self.uniforms.get_mut(i + k * 4..i + k * 4 + 4) {
                word.copy_from_slice(&v.to_le_bytes());
            }
        }
        let o = at as usize + 48;
        for (k, v) in [origin[0] as f32, origin[1] as f32].into_iter().enumerate() {
            if let Some(word) = self.uniforms.get_mut(o + k * 4..o + k * 4 + 4) {
                word.copy_from_slice(&v.to_le_bytes());
            }
        }
        at
    }

    fn composite(
        &mut self,
        rect: [f32; 4],
        size: [u32; 2],
        opacity: f32,
        mode: u32,
        src: [f32; 2],
    ) -> u32 {
        let at = self.slot(&[
            rect[0],
            rect[1],
            rect[2],
            rect[3],
            size[0] as f32,
            size[1] as f32,
            opacity,
            0.0,
            src[0],
            src[1],
        ]);
        let i = at as usize + 28;
        if let Some(word) = self.uniforms.get_mut(i..i + 4) {
            word.copy_from_slice(&mode.to_le_bytes());
        }
        at
    }
}

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    pixels: &Pixels,
) -> GpuTexture {
    let [w, h] = pixels.size;
    let extent = wgpu::Extent3d {
        width: w as u32,
        height: h as u32,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("deesainer texture"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixels.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w as u32 * 4),
            rows_per_image: Some(h as u32),
        },
        extent,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("deesainer texture"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    GpuTexture { bind }
}
