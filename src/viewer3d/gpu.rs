use super::maps::tangents;
use super::trace::{self, Frame, Piece, Shot, Target, Traced, Tracer};
use super::{
    Aabb, BatchKey, Camera, Cut, EMPTY, Image, Look, Material, Painted, Pbr, RenderMode, Scene,
    Style, Trace, V3, View, Viewer, batch_key, batches, depth_range, dot, grow, palette, placed,
    rgb, sub, to_linear,
};
use bytemuck::{Pod, Zeroable};
use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions, Ui};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use web_time::{Duration, Instant};

pub(super) const COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub(super) const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub(super) const SAMPLES: u32 = 4;
const STRIDE: u64 = 512;
const _: () = assert!(std::mem::size_of::<DrawUniform>() <= STRIDE as usize);
const SETTLE: u32 = 16;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    key: [f32; 4],
    fill: [f32; 4],
    key_radiance: [f32; 4],
    fill_radiance: [f32; 4],
    sky: [f32; 4],
    ground: [f32; 4],
    cap: [f32; 4],
    clip: [f32; 4],
    viewport: [f32; 4],
    forward: [f32; 4],
    debug: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    env: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawUniform {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    tint: [f32; 4],
    surface: [f32; 4],
    params: [f32; 4],
    colour: [f32; 4],
    emission: [f32; 4],
    coat: [f32; 4],
    alpha: [f32; 4],
    extra_maps: [f32; 4],
    uv_transform: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    colour: [f32; 3],
    uv: [f32; 2],
    pad: f32,
    tangent: [f32; 4],
}

struct Batch {
    group: usize,
    shading: super::Shading,
    pbr: Pbr,
    texture: Option<usize>,
    normal_map: Option<usize>,
    metal_rough_map: Option<usize>,
    occlusion_map: Option<usize>,
    emission_map: Option<usize>,
    bind: wgpu::BindGroup,
    explicit_colour: bool,
    explicit_material: bool,
    first: u32,
    count: u32,
    aabb: Aabb,
}

struct Stroke {
    group: usize,
    colour: V3,
    width: f32,
    tinted: bool,
    segments: wgpu::Buffer,
    count: u32,
    aabb: Aabb,
}

struct Uploaded {
    used: u64,
    vertices: wgpu::Buffer,
    batches: Vec<Batch>,
    strokes: Vec<Stroke>,
    traced: Option<Traced>,
}

#[derive(Clone, PartialEq)]
struct Snapshot {
    scene: u64,
    camera: Camera,
    looks: Vec<Look>,
    cut: Option<Cut>,
    style: Style,
    trace: Trace,
}

struct Canvas {
    size: [u32; 2],
    resolved: wgpu::Texture,
    resolved_view: wgpu::TextureView,
    multisampled: wgpu::TextureView,
    depth: wgpu::TextureView,
    frame: wgpu::Buffer,
    frame_group: wgpu::BindGroup,
    draws: wgpu::Buffer,
    draw_group: wgpu::BindGroup,
    capacity: u64,
    target: Option<Target>,
    last: Option<Snapshot>,
}

struct Sky {
    used: u64,
    group: wgpu::BindGroup,
    traced: Option<trace::SkyBuffers>,
}

struct Slot {
    used: u64,
    texture: egui::TextureId,
    canvas: Option<Canvas>,
}

struct Pipelines {
    frame_layout: wgpu::BindGroupLayout,
    draw_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    sky_layout: wgpu::BindGroupLayout,
    backdrop: wgpu::RenderPipeline,
    surface_layout: wgpu::PipelineLayout,
    line_layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    surfaces: HashMap<(Fill, i32), wgpu::RenderPipeline>,
    lines: HashMap<bool, wgpu::RenderPipeline>,
}

struct State {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
    pipelines: Pipelines,
    tracer: Option<Tracer>,
    sampler: wgpu::Sampler,
    white: wgpu::TextureView,
    flat: wgpu::TextureView,
    scenes: HashMap<u64, Uploaded>,
    skies: HashMap<u64, Sky>,
    slots: Vec<Slot>,
    pass: u64,
    painted: u64,
    uploads: u64,
    next: usize,
}

struct Soft {
    pass: u64,
    next: usize,
    handles: Vec<TextureHandle>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static SOFT: RefCell<Soft> = const { RefCell::new(Soft { pass: 0, next: 0, handles: Vec::new() }) };
}

static DRAW_MICROS: AtomicU64 = AtomicU64::new(0);

pub fn draw_time() -> Duration {
    Duration::from_micros(DRAW_MICROS.load(Ordering::Relaxed))
}

pub fn wgpu_setup() -> egui_wgpu::WgpuSetup {
    let base = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    let plain = base.device_descriptor.clone();
    egui_wgpu::WgpuSetup::CreateNew(egui_wgpu::WgpuSetupCreateNew {
        device_descriptor: Arc::new(move |adapter| {
            let mut descriptor = plain(adapter);
            let offered = adapter.limits();
            descriptor.required_limits.max_buffer_size = offered.max_buffer_size;
            descriptor.required_limits.max_storage_buffer_binding_size =
                offered.max_storage_buffer_binding_size;
            if adapter.features().contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY) {
                descriptor.required_features |= wgpu::Features::EXPERIMENTAL_RAY_QUERY;
                descriptor.required_limits = descriptor
                    .required_limits
                    .using_acceleration_structure_values(adapter.limits());
                descriptor.experimental_features = unsafe { wgpu::ExperimentalFeatures::enabled() };
            }
            descriptor
        }),
        ..base
    })
}

pub fn install(render_state: &egui_wgpu::RenderState) {
    let device = render_state.device.clone();
    let anisotropic = render_state
        .adapter
        .get_downlevel_capabilities()
        .flags
        .contains(wgpu::DownlevelFlags::ANISOTROPIC_FILTERING);
    let pipelines = Pipelines::new(&device);
    let tracer = trace::supported(&device, &render_state.adapter).then(|| Tracer::new(&device));
    let queue = &render_state.queue;
    let white = image_view(&device, queue, &Image { width: 1, height: 1, rgba: vec![255; 4] });
    let flat =
        image_view(&device, queue, &Image { width: 1, height: 1, rgba: vec![128, 128, 255, 255] });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("viewer3d image"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: if anisotropic { 8 } else { 1 },
        ..Default::default()
    });
    STATE.with(|cell| {
        *cell.borrow_mut() = Some(State {
            queue: render_state.queue.clone(),
            renderer: render_state.renderer.clone(),
            pipelines,
            tracer,
            sampler,
            white,
            flat,
            scenes: HashMap::new(),
            skies: HashMap::new(),
            slots: Vec::new(),
            pass: 0,
            painted: 0,
            uploads: 0,
            next: 0,
            device,
        });
    });
}

pub fn installed() -> bool {
    STATE.with(|cell| cell.borrow().is_some())
}

pub fn hardware_tracing() -> bool {
    STATE.with(|cell| {
        cell.borrow().as_ref().and_then(|s| s.tracer.as_ref()).is_some_and(|t| t.hardware)
    })
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

impl Pipelines {
    fn new(device: &wgpu::Device) -> Pipelines {
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewer3d frame"),
            entries: &[uniform_entry(0, false)],
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewer3d draw"),
            entries: &[uniform_entry(0, true)],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewer3d texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(2, true),
                texture_entry(3, true),
                texture_entry(4, true),
                texture_entry(5, true),
            ],
        });
        let sky_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewer3d sky"),
            entries: &[texture_entry(1, false)],
        });
        let surface_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewer3d surfaces"),
            bind_group_layouts: &[Some(&frame_layout), Some(&draw_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewer3d lines"),
            bind_group_layouts: &[Some(&frame_layout), Some(&draw_layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewer3d"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let backdrop_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewer3d backdrop"),
            bind_group_layouts: &[Some(&frame_layout), Some(&sky_layout)],
            immediate_size: 0,
        });
        let backdrop = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewer3d backdrop"),
            layout: Some(&backdrop_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("backdrop_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("backdrop_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOUR,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        Pipelines {
            frame_layout,
            draw_layout,
            texture_layout,
            sky_layout,
            backdrop,
            surface_layout,
            line_layout,
            module,
            surfaces: HashMap::new(),
            lines: HashMap::new(),
        }
    }

    fn surface(&mut self, device: &wgpu::Device, fill: Fill, bias: i32) -> &wgpu::RenderPipeline {
        let (layout, module) = (&self.surface_layout, &self.module);
        self.surfaces.entry((fill, bias)).or_insert_with(|| {
            let attribute = |shader_location, format, offset| wgpu::VertexAttribute {
                format,
                offset,
                shader_location,
            };
            let attributes = [
                attribute(0, wgpu::VertexFormat::Float32x3, 0),
                attribute(1, wgpu::VertexFormat::Float32x3, 12),
                attribute(2, wgpu::VertexFormat::Float32x3, 24),
                attribute(3, wgpu::VertexFormat::Float32x2, 36),
                attribute(4, wgpu::VertexFormat::Float32x4, 48),
            ];
            pipeline(
                device,
                layout,
                module,
                ("surface_vertex", "surface_fragment"),
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                },
                fill,
                wgpu::DepthBiasState { constant: 1, slope_scale: bias as f32 / 100.0, clamp: 0.0 },
            )
        })
    }

    fn line(&mut self, device: &wgpu::Device, see_through: bool) -> &wgpu::RenderPipeline {
        let (layout, module) = (&self.line_layout, &self.module);
        self.lines.entry(see_through).or_insert_with(|| {
            let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
            pipeline(
                device,
                layout,
                module,
                ("line_vertex", "line_fragment"),
                wgpu::VertexBufferLayout {
                    array_stride: 24,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                },
                if see_through { Fill::SeeThrough } else { Fill::Opaque },
                wgpu::DepthBiasState::default(),
            )
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Fill {
    Opaque,
    SeeThrough,
    DepthOnly,
    Additive,
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    (vertex, fragment): (&str, &str),
    buffer: wgpu::VertexBufferLayout,
    fill: Fill,
    bias: wgpu::DepthBiasState,
) -> wgpu::RenderPipeline {
    let keep = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    };
    let blend = match fill {
        Fill::SeeThrough => Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: keep,
        }),
        Fill::Additive => Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: keep,
        }),
        Fill::Opaque | Fill::DepthOnly => None,
    };
    let write_mask = match fill {
        Fill::DepthOnly => wgpu::ColorWrites::empty(),
        _ => wgpu::ColorWrites::ALL,
    };
    let (depth_write, depth_compare) = match fill {
        Fill::Opaque | Fill::DepthOnly => (true, wgpu::CompareFunction::Less),
        Fill::SeeThrough => (false, wgpu::CompareFunction::Less),
        Fill::Additive => (false, wgpu::CompareFunction::Always),
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(vertex),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vertex),
            compilation_options: Default::default(),
            buffers: &[Some(buffer)],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format: COLOUR, blend, write_mask })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(depth_write),
            depth_compare: Some(depth_compare),
            stencil: Default::default(),
            bias,
        }),
        multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

fn mip_levels(image: &Image) -> Vec<(u32, u32, Vec<u8>)> {
    let mut levels = vec![(image.width as u32, image.height as u32, image.rgba.clone())];
    while levels.len() < 12 {
        let (w, h, data) = levels.last().expect("first level");
        if *w == 1 && *h == 1 {
            break;
        }
        let (nw, nh) = ((*w / 2).max(1), (*h / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for k in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(w - 1);
                        let sy = (y * 2 + dy).min(h - 1);
                        sum += data[((sy * w + sx) * 4 + k) as usize] as u32;
                    }
                    next[((y * nw + x) * 4 + k) as usize] = ((sum + 2) / 4) as u8;
                }
            }
        }
        levels.push((nw, nh, next));
    }
    levels
}

fn image_view(device: &wgpu::Device, queue: &wgpu::Queue, image: &Image) -> wgpu::TextureView {
    let image = if image.width == 0
        || image.height == 0
        || image.rgba.len() < image.width * image.height * 4
    {
        &Image { width: 1, height: 1, rgba: vec![255; 4] }
    } else {
        image
    };
    let levels = mip_levels(image);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("viewer3d image"),
        size: wgpu::Extent3d {
            width: image.width as u32,
            height: image.height as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOUR,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (w, h, data)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(*h),
            },
            wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
        );
    }
    texture.create_view(&Default::default())
}

fn material_group(
    state: &State,
    views: &[wgpu::TextureView],
    maps: [Option<usize>; 5],
    explicit_lod: Option<u32>,
) -> wgpu::BindGroup {
    let view = |m: Option<usize>, blank| m.and_then(|i| views.get(i)).unwrap_or(blank);
    let [colour, normal, metal_rough, occlusion, emission] = maps;
    let entries = [
        (0, view(colour, &state.white)),
        (2, view(normal, &state.flat)),
        (3, view(metal_rough, &state.white)),
        (4, view(occlusion, &state.white)),
        (5, view(emission, &state.white)),
    ];
    let mut bound: Vec<wgpu::BindGroupEntry> = entries
        .iter()
        .map(|(binding, view)| wgpu::BindGroupEntry {
            binding: *binding,
            resource: wgpu::BindingResource::TextureView(view),
        })
        .collect();
    let explicit_sampler = explicit_lod.map(|lod| super::explicit_image::explicit_sampler(&state.device, lod));
    bound.push(wgpu::BindGroupEntry {
        binding: 1,
        resource: wgpu::BindingResource::Sampler(explicit_sampler.as_ref().unwrap_or(&state.sampler)),
    });
    state.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("viewer3d material"),
        layout: &state.pipelines.texture_layout,
        entries: &bound,
    })
}

fn four(v: V3, w: f32) -> [f32; 4] {
    [v[0], v[1], v[2], w]
}

fn vertex_buffer(state: &State, label: &str, bytes: &[u8]) -> wgpu::Buffer {
    let buffer = state.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (bytes.len() as u64).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT).max(4),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    state.queue.write_buffer(&buffer, 0, bytes);
    buffer
}

struct Gathered<M> {
    key: BatchKey<M>,
    vertices: Vec<Vertex>,
    aabb: Aabb,
}

fn gather<M: Material>(scene: &Scene<M>) -> Vec<Gathered<M>> {
    let (ids, count) = batches(scene);
    let mut out: Vec<Option<Gathered<M>>> = (0..count).map(|_| None).collect();
    for (s, id) in scene.surfaces.iter().zip(ids) {
        let Some(id) = id else { continue };
        let key = batch_key(scene, s);
        let g = out[id].get_or_insert_with(|| Gathered { key, vertices: Vec::new(), aabb: EMPTY });
        let colour = s.colour.map(|c| to_linear((c.clamp(0.0, 1.0) * 255.0).round() / 255.0));
        let tangents = if key.2[1].is_some() { tangents(s) } else { Vec::new() };
        for (i, p) in s.positions.iter().take(s.positions.len() / 3 * 3).enumerate() {
            grow(&mut g.aabb, *p);
            g.vertices.push(Vertex {
                position: *p,
                normal: s.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]),
                colour,
                uv: s.uvs.get(i).copied().unwrap_or_default(),
                pad: 0.0,
                tangent: tangents.get(i).copied().unwrap_or_default(),
            });
        }
    }
    out.into_iter().flatten().collect()
}

fn upload<M: Material>(state: &State, scene: &Scene<M>) -> Uploaded {
    scene.validate_explicit_images().expect("invalid explicit scene images");
    let device = &state.device;
    let views: Vec<wgpu::TextureView> =
        scene.images.iter().enumerate().map(|(index, image)| {
            match scene.explicit_images.get(&index) {
                Some(explicit) => explicit.upload(device, &state.queue).expect("validated explicit GPU image").view,
                None => image_view(device, &state.queue, image),
            }
        }).collect();
    let gathered = gather(scene);
    let mut all: Vec<Vertex> = Vec::new();
    let mut batches = Vec::with_capacity(gathered.len());
    let limits = device.limits();
    let budget = if state.tracer.is_some() {
        limits.max_buffer_size.min(limits.max_storage_buffer_binding_size)
    } else {
        limits.max_buffer_size
    };
    let fits = (budget / std::mem::size_of::<Vertex>() as u64) as usize;
    let mut dropped = 0usize;
    let mut full = false;
    for g in gathered {
        full = full || all.len() + g.vertices.len() > fits;
        if full {
            dropped += g.vertices.len() / 3;
            continue;
        }
        let (group, material, maps) = g.key;
        let [texture, normal_map, metal_rough_map, occlusion_map, emission_map] = maps;
        batches.push(Batch {
            group,
            shading: material.shading(),
            pbr: material.pbr(),
            texture,
            normal_map,
            metal_rough_map,
            occlusion_map,
            emission_map,
            bind: material_group(state, &views, maps, {
                let explicit = maps.map(|i| i.and_then(|i| scene.explicit_images.get(&i)));
                explicit.iter().any(Option::is_some).then(||
                    super::explicit_image::explicit_material_lod_with_extras(explicit).expect("validated material"))
            }),
            explicit_colour: texture.is_some_and(|i| scene.explicit_images.contains_key(&i)),
            explicit_material: maps.iter().flatten().any(|i| scene.explicit_images.contains_key(i)),
            first: all.len() as u32,
            count: g.vertices.len() as u32,
            aabb: g.aabb,
        });
        all.extend(g.vertices);
    }
    if dropped > 0 {
        eprintln!(
            "viewer3d: scene needs more than this GPU's {} MiB buffer limit, leaving out {dropped} of {} triangles",
            budget >> 20,
            dropped + all.len() / 3
        );
    }
    let mut usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
    if let Some(tracer) = &state.tracer {
        usage |= wgpu::BufferUsages::STORAGE;
        if tracer.hardware {
            usage |= wgpu::BufferUsages::BLAS_INPUT;
        }
    }
    let bytes: &[u8] = bytemuck::cast_slice(&all);
    let vertices = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("viewer3d surfaces"),
        size: (bytes.len() as u64).next_multiple_of(16).max(48),
        usage,
        mapped_at_creation: false,
    });
    state.queue.write_buffer(&vertices, 0, bytes);
    let strokes = scene
        .lines
        .iter()
        .filter(|l| !l.segments.is_empty())
        .map(|l| {
            let mut aabb = EMPTY;
            for [a, b] in &l.segments {
                grow(&mut aabb, *a);
                grow(&mut aabb, *b);
            }
            Stroke {
                group: l.group,
                colour: l.colour,
                width: l.width,
                tinted: l.material.shading().tinted,
                count: l.segments.len() as u32,
                segments: vertex_buffer(state, "viewer3d lines", bytemuck::cast_slice(&l.segments)),
                aabb,
            }
        })
        .collect();
    Uploaded { used: 0, vertices, batches, strokes, traced: None }
}

fn trace_scene<M: Material>(
    state: &State,
    tracer: &Tracer,
    scene: &Scene<M>,
    uploaded: &Uploaded,
) -> Traced {
    let gathered = gather(scene);
    let triangles: Vec<Vec<[V3; 3]>> = gathered
        .iter()
        .map(|g| {
            g.vertices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| [t[0].position, t[1].position, t[2].position])
                .collect()
        })
        .collect();
    let pieces: Vec<Piece> = uploaded
        .batches
        .iter()
        .zip(&triangles)
        .map(|(b, triangles)| Piece {
            first: b.first,
            count: b.count,
            maps: [b.texture, b.normal_map, b.metal_rough_map],
            shading: b.shading,
            pbr: b.pbr,
            triangles,
        })
        .collect();
    Traced::new(tracer, &state.device, &state.queue, &uploaded.vertices, &pieces, &scene.images)
}

pub(super) fn columns(m: &[f32; 16]) -> [[f32; 4]; 4] {
    [
        [m[0], m[1], m[2], m[3]],
        [m[4], m[5], m[6], m[7]],
        [m[8], m[9], m[10], m[11]],
        [m[12], m[13], m[14], m[15]],
    ]
}

pub(super) fn normal_matrix(m: &[f32; 16]) -> [[f32; 4]; 4] {
    let a = [[m[0], m[4], m[8]], [m[1], m[5], m[9]], [m[2], m[6], m[10]]];
    let cof = |r: usize, c: usize| {
        let (r0, r1) = ((r + 1) % 3, (r + 2) % 3);
        let (c0, c1) = ((c + 1) % 3, (c + 2) % 3);
        a[r0][c0] * a[r1][c1] - a[r0][c1] * a[r1][c0]
    };
    let det = a[0][0] * cof(0, 0) + a[0][1] * cof(0, 1) + a[0][2] * cof(0, 2);
    let inv = if det.abs() > 1.0e-20 { 1.0 / det } else { 1.0 };
    let n = |r: usize, c: usize| cof(r, c) * inv;
    [
        [n(0, 0), n(1, 0), n(2, 0), 0.0],
        [n(0, 1), n(1, 1), n(2, 1), 0.0],
        [n(0, 2), n(1, 2), n(2, 2), 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn view_projection(vw: &View, size: [u32; 2], ortho: bool, near: f32, far: f32) -> [[f32; 4]; 4] {
    let (w, h) = (size[0] as f32, size[1] as f32);
    let row = |axis: V3| [axis[0], axis[1], axis[2], -dot(axis, vw.eye)];
    let (x, y, d) = (row(vw.right), row(vw.up), row(vw.forward));
    let one = [0.0, 0.0, 0.0, 1.0];
    let scaled = |r: [f32; 4], k: f32| r.map(|v| v * k);
    let span = (far - near).max(1.0e-12);
    let rows = if ortho {
        let half_h = h * 0.5 * vw.dist / vw.focal;
        let half_w = half_h * w / h;
        let z = std::array::from_fn(|i| (d[i] - near * one[i]) / span);
        [scaled(x, 1.0 / half_w), scaled(y, 1.0 / half_h), z, one]
    } else {
        let t = (vw.fov_y * 0.5).tan();
        let z = std::array::from_fn(|i| far / span * d[i] - far * near / span * one[i]);
        [scaled(x, 1.0 / (t * w / h)), scaled(y, 1.0 / t), z, d]
    };
    std::array::from_fn(|c| std::array::from_fn(|r| rows[r][c]))
}

impl State {
    fn begin(&mut self, pass: u64) {
        if pass == self.pass {
            return;
        }
        self.pass = pass;
        self.painted += 1;
        self.next = 0;
        let painted = self.painted;
        self.scenes.retain(|_, u| u.used + 1 >= painted);
        self.skies.retain(|_, s| s.used + 1 >= painted);
        let mut renderer = self.renderer.write();
        self.slots.retain(|s| {
            let keep = s.used + 1 >= painted;
            if !keep {
                renderer.free_texture(&s.texture);
            }
            keep
        });
    }

    fn canvas(&self, size: [u32; 2]) -> Canvas {
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let make = |label, format, samples, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let resolved = make(
            "viewer3d resolved",
            COLOUR,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        let multisampled =
            make("viewer3d multisampled", COLOUR, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let depth = make("viewer3d depth", DEPTH, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let frame = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewer3d frame"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewer3d frame"),
            layout: &self.pipelines.frame_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame.as_entire_binding() }],
        });
        let (draws, draw_group) = self.draw_buffer(16);
        Canvas {
            size,
            resolved_view: resolved.create_view(&Default::default()),
            resolved,
            multisampled: multisampled.create_view(&Default::default()),
            depth: depth.create_view(&Default::default()),
            frame,
            frame_group,
            draws,
            draw_group,
            capacity: 16,
            target: None,
            last: None,
        }
    }

    fn slot(&mut self, size: [u32; 2]) -> usize {
        let index = self.next;
        self.next += 1;
        let fits = self
            .slots
            .get(index)
            .is_some_and(|s| s.canvas.as_ref().is_some_and(|c| c.size == size));
        if !fits {
            let canvas = self.canvas(size);
            let mut renderer = self.renderer.write();
            match self.slots.get_mut(index) {
                Some(slot) => {
                    renderer.update_egui_texture_from_wgpu_texture(
                        &self.device,
                        &canvas.resolved_view,
                        wgpu::FilterMode::Nearest,
                        slot.texture,
                    );
                    slot.canvas = Some(canvas);
                }
                None => {
                    let texture = renderer.register_native_texture(
                        &self.device,
                        &canvas.resolved_view,
                        wgpu::FilterMode::Nearest,
                    );
                    drop(renderer);
                    self.slots.push(Slot { used: self.painted, texture, canvas: Some(canvas) });
                }
            }
        }
        self.slots[index].used = self.painted;
        index
    }

    fn draw_buffer(&self, capacity: u64) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewer3d draws"),
            size: capacity * STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewer3d draws"),
            layout: &self.pipelines.draw_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<DrawUniform>() as u64),
                }),
            }],
        });
        (buffer, group)
    }

    fn prepare<M: Material>(&mut self, viewer: &Viewer<M>, traced: bool) {
        let scene = &viewer.scene;
        if !self.scenes.contains_key(&scene.id) {
            let uploaded = upload(self, scene);
            self.scenes.insert(scene.id, uploaded);
            self.uploads += 1;
        }
        let uploaded = &self.scenes[&scene.id];
        if traced
            && uploaded.traced.is_none()
            && !uploaded.batches.is_empty()
            && let Some(tracer) = &self.tracer
        {
            let made = trace_scene(self, tracer, scene, uploaded);
            self.scenes.get_mut(&scene.id).expect("uploaded").traced = Some(made);
        }
        self.scenes.get_mut(&scene.id).expect("uploaded").used = self.painted;
        if let Some(light) = &viewer.environment {
            let map = &light.map;
            if !self.skies.contains_key(&map.id()) {
                let sky = Sky { used: 0, group: self.sky_group(map), traced: None };
                self.skies.insert(map.id(), sky);
            }
            let sky = self.skies.get_mut(&map.id()).expect("sky");
            sky.used = self.painted;
            if traced
                && sky.traced.is_none()
                && let Some(tracer) = &self.tracer
            {
                sky.traced = Some(tracer.sky(&self.device, &self.queue, map));
            }
        }
    }

    fn sky_group(&self, map: &super::Environment) -> wgpu::BindGroup {
        let (w, h) = (map.width() as u32, map.height() as u32);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewer3d sky"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texels: Vec<[f32; 4]> = map.rgb().iter().map(|c| [c[0], c[1], c[2], 1.0]).collect();
        self.queue.write_texture(
            texture.as_image_copy(),
            bytemuck::cast_slice(&texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 16),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&Default::default());
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewer3d sky"),
            layout: &self.pipelines.sky_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        })
    }
}

enum Kind {
    Batch(usize),
    Stroke(usize),
}

struct Item {
    kind: Kind,
    see_through: bool,
    distance: f32,
}

enum Mode {
    Raster,
    Live(Trace),
    Offline(Trace),
}

fn tracing(state: &State, viewer: &Viewer<impl Material>, mode: &Mode) -> Option<Trace> {
    let trace = match mode {
        Mode::Raster => None,
        Mode::Live(t) | Mode::Offline(t) => Some(*t),
    }?;
    let uploaded = state.scenes.get(&viewer.scene.id)?;
    (state.tracer.is_some() && uploaded.traced.is_some()).then_some(Trace {
        samples: trace.samples.max(1),
        per_frame: trace.per_frame.max(1),
        bounces: trace.bounces.max(1),
    })
}

fn draw<M: Material>(
    state: &mut State,
    canvas: &mut Canvas,
    viewer: &Viewer<M>,
    ppp: f32,
    mode: Mode,
) -> Painted {
    let wants = !matches!(mode, Mode::Raster);
    state.prepare(viewer, wants);
    let trace = tracing(state, viewer, &mode);
    let size = canvas.size;
    let scene = &viewer.scene;
    let style: Style = viewer.style;
    let vw = viewer.view(egui::Vec2::new(size[0] as f32, size[1] as f32));
    let selected = rgb(style.selected);
    let mut draws: Vec<DrawUniform> = Vec::new();
    let mut items: Vec<Item> = Vec::new();
    let mut corners: Vec<[V3; 8]> = Vec::new();
    let mut widest: f32 = 0.0;
    let uploaded = &state.scenes[&scene.id];
    let distance = |box_: &[V3; 8]| {
        let centre = box_
            .iter()
            .fold([0.0; 3], |a, p| [a[0] + p[0] / 8.0, a[1] + p[1] / 8.0, a[2] + p[2] / 8.0]);
        let d = sub(centre, vw.eye);
        dot(d, d)
    };
    for (i, batch) in uploaded.batches.iter().enumerate() {
        let look = viewer.look(batch.group);
        if !look.visible() {
            continue;
        }
        let opacity = look.opacity.min(1.0) * batch.pbr.raster_opacity();
        let tint = if look.selected && batch.shading.tinted {
            [selected[0], selected[1], selected[2], style.selected_tint]
        } else {
            [0.0; 4]
        };
        let (s, p) = (batch.shading, batch.pbr);
        let flag = |on: bool| if on { 1.0 } else { 0.0 };
        draws.push(DrawUniform {
            uv_transform: p.uv_transform,
            model: columns(&look.placement),
            normal: normal_matrix(&look.placement),
            tint,
            surface: [p.roughness, p.metallic, p.specular, flag(s.capped)],
            params: [opacity, if batch.explicit_colour { 2.0 } else { flag(batch.texture.is_some()) }, 0.0, flag(batch.normal_map.is_some())],
            colour: match viewer.mode {
                RenderMode::Groups => four(palette(batch.group), 1.0),
                RenderMode::Batches => four(palette(i), 1.0),
                _ => [0.0; 4],
            },
            emission: four(p.emission, p.normal_scale),
            extra_maps: [flag(batch.occlusion_map.is_some()), p.occlusion_strength,
                if batch.emission_map.is_none() { 0.0 } else if batch.explicit_material { 2.0 } else { 1.0 }, 0.0],
            coat: [p.clearcoat, p.clearcoat_roughness, flag(batch.metal_rough_map.is_some()), flag(batch.explicit_material)],
            alpha: match p.alpha {
                super::AlphaMode::Opaque => [0.0, 1.0, 0.0, 0.0],
                super::AlphaMode::Blend => [1.0, p.base_alpha, 0.0, 0.0],
                super::AlphaMode::Mask { cutoff } => [2.0, p.base_alpha, cutoff, 0.0],
            },
        });
        let box_ = placed(&batch.aabb, &look);
        items.push(Item {
            kind: Kind::Batch(i),
            see_through: opacity < 1.0 || p.alpha == super::AlphaMode::Blend,
            distance: distance(&box_),
        });
        corners.push(box_);
    }
    for (i, stroke) in uploaded.strokes.iter().enumerate() {
        let look = viewer.look(stroke.group);
        if !look.visible() || !viewer.edges {
            continue;
        }
        let opacity = (look.opacity * 1.5).min(1.0);
        let colour = if look.selected && stroke.tinted { selected } else { stroke.colour };
        let pixels = stroke.width * ppp;
        widest = widest.max(pixels);
        draws.push(DrawUniform {
            model: columns(&look.placement),
            normal: normal_matrix(&look.placement),
            tint: [0.0; 4],
            surface: [0.0; 4],
            params: [opacity, 0.0, pixels, 0.0],
            colour: [colour[0], colour[1], colour[2], 1.0],
            emission: [0.0; 4],
            extra_maps: [0.0; 4],
            uv_transform: [1.,1.,0.,0.],
            coat: [0.0; 4],
            alpha: [0.0; 4],
        });
        let box_ = placed(&stroke.aabb, &look);
        items.push(Item {
            kind: Kind::Stroke(i),
            see_through: opacity < 1.0,
            distance: distance(&box_),
        });
        corners.push(box_);
    }
    let (near, far) = depth_range(&corners, &vw, viewer.camera.ortho);
    let clip = viewer.cut.map(|c| [c.normal[0], c.normal[1], c.normal[2], c.offset]);
    let lighting = &style.lighting;
    let frame = FrameUniform {
        view_proj: view_projection(&vw, size, viewer.camera.ortho, near, far),
        eye: [vw.eye[0], vw.eye[1], vw.eye[2], 1.0],
        key: four(lighting.direction(&lighting.key, &vw), lighting.exposure),
        fill: four(lighting.direction(&lighting.fill, &vw), 0.0),
        key_radiance: four(lighting.key.radiance(), 0.0),
        fill_radiance: four(lighting.fill.radiance(), 0.0),
        sky: four(viewer.ambient().0, 0.0),
        ground: four(viewer.ambient().1, 0.0),
        cap: {
            let c = rgb(style.cap);
            [c[0], c[1], c[2], 1.0]
        },
        clip: clip.unwrap_or([0.0, 0.0, 1.0, 0.0]),
        viewport: [size[0] as f32, size[1] as f32, if clip.is_some() { 1.0 } else { 0.0 }, 0.0],
        forward: four(vw.forward, 0.0),
        debug: [viewer.mode as u32 as f32, near, far, ppp.max(1.0)],
        right: four(vw.right, vw.focal),
        up: four(vw.up, if viewer.camera.ortho { vw.dist } else { 0.0 }),
        env: viewer
            .environment
            .as_ref()
            .map_or([0.0; 4], |e| [e.turn, e.strength, if e.backdrop { 1.0 } else { 0.0 }, 1.0]),
    };
    let bias = ((widest * 0.5 + 1.0) * 100.0).round() as i32;
    if canvas.capacity < draws.len() as u64 {
        let capacity = (draws.len() as u64).next_power_of_two();
        let (buffer, group) = state.draw_buffer(capacity);
        canvas.draws = buffer;
        canvas.draw_group = group;
        canvas.capacity = capacity;
    }
    let device = state.device.clone();
    let queue = state.queue.clone();
    let mut encoder = device.create_command_encoder(&Default::default());
    let mut painted = Painted {
        hardware: state.tracer.as_ref().is_some_and(|t| t.hardware),
        ..Painted::default()
    };
    let mut present = None;
    if let Some(trace) = trace {
        let snapshot = Snapshot {
            scene: scene.id,
            camera: viewer.camera,
            looks: viewer.looks.to_vec(),
            cut: viewer.cut,
            style,
            trace,
        };
        if canvas.target.as_ref().is_none_or(|t| t.size() != size) {
            canvas.target = Some(Target::new(&device, size));
            canvas.last = None;
        }
        let target = canvas.target.as_mut().expect("made above");
        let offline = matches!(mode, Mode::Offline(_));
        let heat = viewer.mode == RenderMode::Traversal;
        let moved = canvas.last.as_ref() != Some(&snapshot);
        if moved {
            canvas.last = Some(snapshot);
            target.samples = 0;
            target.stamp = target.stamp.wrapping_add(1);
        }
        let tracer = state.tracer.as_ref().expect("tracing");
        let Uploaded { vertices, batches, traced, .. } =
            state.scenes.get_mut(&scene.id).expect("uploaded");
        let traced = traced.as_mut().expect("traced");
        let looks: Vec<Look> = batches.iter().map(|b| viewer.look(b.group)).collect();
        let sky_buffers = viewer
            .environment
            .as_ref()
            .and_then(|e| state.skies.get(&e.map.id()))
            .and_then(|s| s.traced.as_ref());
        let shot = Shot {
            view: &vw,
            ortho: viewer.camera.ortho,
            style: &style,
            cut: viewer.cut,
            trace,
            radius: scene.radius,
            heat,
            size,
            instances: looks.len() as u32,
            ambient: viewer.ambient(),
            sky: sky_buffers.zip(viewer.environment.as_ref()).map(|(b, e)| trace::Sky {
                turn: e.turn,
                strength: e.strength,
                backdrop: e.backdrop,
                size: b.size,
            }),
        };
        let frame_data = Frame { looks, style: &style, vertices, sky: sky_buffers };
        if offline {
            while target.samples < trace.samples {
                let count = trace.per_frame.min(trace.samples - target.samples);
                let mut chunk = device.create_command_encoder(&Default::default());
                tracer.dispatch(
                    &device,
                    &queue,
                    &mut chunk,
                    traced,
                    &frame_data,
                    target,
                    &shot,
                    count,
                );
                queue.submit([chunk.finish()]);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
            }
        } else if (heat || !moved) && target.samples < trace.samples {
            let count = trace.per_frame.min(trace.samples - target.samples);
            tracer.dispatch(
                &device,
                &queue,
                &mut encoder,
                traced,
                &frame_data,
                target,
                &shot,
                count,
            );
        }
        painted.target = trace.samples;
        if target.samples >= SETTLE.min(trace.samples) {
            painted.samples = target.samples;
            present = Some(tracer.present_group(&device, &queue, target));
        }
    } else {
        canvas.target = None;
        canvas.last = None;
    }
    let uploaded = &state.scenes[&scene.id];
    let mut order: Vec<usize> = (0..items.len()).filter(|i| !items[*i].see_through).collect();
    let mut behind: Vec<usize> = (0..items.len()).filter(|i| items[*i].see_through).collect();
    behind.sort_by(|a, b| items[*b].distance.total_cmp(&items[*a].distance));
    order.extend(behind);
    if present.is_some() {
        order.retain(|i| matches!(items[*i].kind, Kind::Stroke(_)) || !items[*i].see_through);
    }
    let fill = |item: &Item| match (present.is_some(), viewer.mode, item.see_through) {
        (true, _, _) => Fill::DepthOnly,
        (_, RenderMode::Overdraw, _) => Fill::Additive,
        (_, _, true) => Fill::SeeThrough,
        _ => Fill::Opaque,
    };
    for item in &items {
        match item.kind {
            Kind::Batch(_) => {
                state.pipelines.surface(&device, fill(item), bias);
            }
            Kind::Stroke(_) => {
                state.pipelines.line(&device, item.see_through);
            }
        }
    }
    queue.write_buffer(&canvas.frame, 0, bytemuck::bytes_of(&frame));
    let mut packed = vec![0u8; draws.len() * STRIDE as usize];
    for (k, d) in draws.iter().enumerate() {
        let at = k * STRIDE as usize;
        packed[at..at + std::mem::size_of::<DrawUniform>()].copy_from_slice(bytemuck::bytes_of(d));
    }
    if !packed.is_empty() {
        queue.write_buffer(&canvas.draws, 0, &packed);
    }
    let [r, g, b] = rgb(style.background);
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("viewer3d"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &canvas.multisampled,
                depth_slice: None,
                resolve_target: Some(&canvas.resolved_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: r as f64,
                        g: g as f64,
                        b: b as f64,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &canvas.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let (Some(group), Some(tracer)) = (&present, &state.tracer) {
            pass.set_pipeline(&tracer.present);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
        pass.set_bind_group(0, &canvas.frame_group, &[]);
        if present.is_none()
            && let Some(light) = viewer.environment.as_ref().filter(|e| e.backdrop)
            && let Some(sky) = state.skies.get(&light.map.id())
        {
            pass.set_pipeline(&state.pipelines.backdrop);
            pass.set_bind_group(1, &sky.group, &[]);
            pass.draw(0..3, 0..1);
        }
        for i in order {
            let item = &items[i];
            let offset = (i as u64 * STRIDE) as u32;
            pass.set_bind_group(1, &canvas.draw_group, &[offset]);
            match item.kind {
                Kind::Batch(b) => {
                    let batch = &uploaded.batches[b];
                    let key = (fill(item), bias);
                    pass.set_pipeline(&state.pipelines.surfaces[&key]);
                    pass.set_bind_group(2, &batch.bind, &[]);
                    pass.set_vertex_buffer(0, uploaded.vertices.slice(..));
                    pass.draw(batch.first..batch.first + batch.count, 0..1);
                }
                Kind::Stroke(s) => {
                    let stroke = &uploaded.strokes[s];
                    pass.set_pipeline(&state.pipelines.lines[&item.see_through]);
                    pass.set_vertex_buffer(0, stroke.segments.slice(..));
                    pass.draw(0..6, 0..stroke.count);
                }
            }
        }
    }
    queue.submit([encoder.finish()]);
    painted
}

const HEAT: Trace = Trace { samples: 4, bounces: 1, per_frame: 4 };

fn pass_for<M: Material>(viewer: &Viewer<M>) -> Option<Trace> {
    match viewer.mode {
        RenderMode::Traversal => Some(HEAT),
        RenderMode::Shaded => viewer.trace,
        _ => None,
    }
}

fn render<M: Material>(
    state: &mut State,
    viewer: &Viewer<M>,
    size: [u32; 2],
    ppp: f32,
) -> (egui::TextureId, Painted) {
    let index = state.slot(size);
    let mut canvas = state.slots[index].canvas.take().expect("slot canvas");
    let mode = pass_for(viewer).map_or(Mode::Raster, Mode::Live);
    let painted = draw(state, &mut canvas, viewer, ppp, mode);
    state.slots[index].canvas = Some(canvas);
    (state.slots[index].texture, painted)
}

pub(super) fn render_gpu<M: Material>(
    viewer: &Viewer<M>,
    width: usize,
    height: usize,
) -> Option<ColorImage> {
    STATE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let state = slot.as_mut()?;
        let trace = pass_for(viewer);
        if trace.is_some() {
            state.tracer.as_ref()?;
        }
        let size = [width.max(1) as u32, height.max(1) as u32];
        let mut canvas = state.canvas(size);
        let mode = trace.map_or(Mode::Raster, Mode::Offline);
        draw(state, &mut canvas, viewer, 1.0, mode);
        Some(read_back(state, &canvas))
    })
}

fn read_back(state: &State, canvas: &Canvas) -> ColorImage {
    let [w, h] = canvas.size;
    let row = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = state.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("viewer3d read back"),
        size: row as u64 * h as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = state.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        canvas.resolved.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    state.queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = state.device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().expect("readback buffer is mapped");
    let pixels = (0..h as usize)
        .flat_map(|y| {
            let line = &data[y * row as usize..][..w as usize * 4];
            line.as_chunks::<4>()
                .0
                .iter()
                .map(|p| Color32::from_rgb(p[0], p[1], p[2]))
                .collect::<Vec<_>>()
        })
        .collect();
    drop(data);
    buffer.unmap();
    ColorImage::new([w as usize, h as usize], pixels)
}

fn soft<M: Material>(ui: &Ui, rect: Rect, viewer: &Viewer<M>, size: [u32; 2]) -> egui::TextureId {
    let image = viewer.render_soft(size[0] as usize, size[1] as usize);
    let pass = ui.ctx().cumulative_pass_nr();
    SOFT.with(|cell| {
        let mut soft = cell.borrow_mut();
        if soft.pass != pass {
            soft.pass = pass;
            soft.next = 0;
        }
        let index = soft.next;
        soft.next += 1;
        if let Some(handle) = soft.handles.get_mut(index) {
            handle.set(image, TextureOptions::NEAREST);
        } else {
            let handle = ui.ctx().load_texture(
                format!("viewer3d {index} {rect:?}"),
                image,
                TextureOptions::NEAREST,
            );
            soft.handles.push(handle);
        }
        soft.handles[index].id()
    })
}

pub(super) fn paint<M: Material>(ui: &Ui, rect: Rect, viewer: &Viewer<M>) -> Painted {
    let started = Instant::now();
    let ppp = ui.ctx().pixels_per_point();
    let size = [
        (rect.width() * ppp).round().max(1.0) as u32,
        (rect.height() * ppp).round().max(1.0) as u32,
    ];
    let pass = ui.ctx().cumulative_pass_nr();
    let drawn = STATE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let state = slot.as_mut()?;
        state.begin(pass);
        Some(render(state, viewer, size, ppp))
    });
    let (texture, painted) =
        drawn.unwrap_or_else(|| (soft(ui, rect, viewer, size), Painted::default()));
    if painted.target > 0 && !painted.settled() {
        ui.ctx().request_repaint();
    }
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    ui.painter_at(rect).image(texture, rect, uv, Color32::WHITE);
    DRAW_MICROS.store(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    painted
}

#[cfg(test)]
pub(super) fn uploads() -> u64 {
    STATE.with(|cell| cell.borrow().as_ref().map_or(0, |s| s.uploads))
}

#[cfg(test)]
mod tests {
    use super::super::{Camera, Scene, Surface, Viewer};
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};

    #[test]
    fn a_scene_stays_on_the_gpu_while_passes_go_by_unpainted() {
        let made = std::panic::catch_unwind(|| {
            create_render_state(default_wgpu_setup(), egui_wgpu::RendererOptions::PREDICTABLE)
        });
        let Ok(state) = made else {
            eprintln!("skipped: no wgpu adapter");
            return;
        };
        install(&state);
        let triangle: Surface = Surface {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            ..Default::default()
        };
        let scene = Arc::new(Scene { surfaces: vec![triangle], ..Default::default() });
        let viewer = Viewer::new(scene, Camera::default());
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 48.0));
        let mut pass = 0;
        let mut h = Harness::builder()
            .with_size(rect.size())
            .renderer(WgpuTestRenderer::from_render_state(state))
            .build_ui(move |ui| {
                pass += 1;
                if pass % 3 == 0 {
                    viewer.paint(ui, rect);
                }
            });
        for _ in 0..12 {
            h.step();
        }
        assert_eq!(uploads(), 1);
    }
}
