mod controls;
#[cfg(test)]
mod explicit_scene_tests;
#[cfg(test)]
mod alpha_scene_tests;
#[cfg(test)]
mod extra_maps_tests;
mod explicit_image;
pub use explicit_image::{ExplicitImage, ImageEncoding, UploadedImage};
mod environment;
mod gpu;
mod maps;
mod soft;
mod trace;

use egui::{Color32, ColorImage, Pos2, Rect, Response, Ui, Vec2};
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub use controls::{lighting_controls, mode_controls, preset_controls, trace_controls};
pub use environment::{Environment, EnvironmentLight};
pub use gpu::{draw_time, hardware_tracing, install, installed, wgpu_setup};

pub type V3 = [f32; 3];
pub type Placement = [f32; 16];

pub const IDENTITY: Placement =
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
pub const BACKGROUND: Color32 = Color32::from_rgb(20, 22, 26);
const DISTANCE: f32 = 2.6;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shading {
    pub gloss: f32,
    pub sharpness: f32,
    pub metal: f32,
    pub capped: bool,
    pub tinted: bool,
}

impl Default for Shading {
    fn default() -> Self {
        Shading { gloss: 0.12, sharpness: 40.0, metal: 0.0, capped: false, tinted: true }
    }
}

impl Shading {
    pub fn pbr(&self) -> Pbr {
        Pbr {
            roughness: (2.0 / (self.sharpness.max(0.0) + 2.0)).powf(0.25),
            metallic: self.metal.clamp(0.0, 1.0),
            specular: self.gloss.max(0.0) / 0.24,
            ..Pbr::default()
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AlphaMode {
    Opaque,
    Blend,
    Mask { cutoff: f32 },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pbr {
    /// Shared texture sampling transform: scale U/V, offset U/V. GPU raster only.
    pub uv_transform: [f32; 4],
    /// Texture alpha and base_alpha are ignored in Opaque mode.
    pub alpha: AlphaMode,
    pub base_alpha: f32,
    pub roughness: f32,
    pub metallic: f32,
    pub specular: f32,
    pub emission: V3,
    pub occlusion_strength: f32,
    pub transmission: f32,
    pub ior: f32,
    pub clearcoat: f32,
    pub clearcoat_roughness: f32,
    pub normal_scale: f32,
}

impl Default for Pbr {
    fn default() -> Self {
        Pbr {
            alpha: AlphaMode::Opaque,
            uv_transform: [1., 1., 0., 0.],
            base_alpha: 1.0,
            roughness: 0.5,
            metallic: 0.0,
            specular: 0.5,
            emission: [0.0; 3],
            occlusion_strength: 1.0,
            transmission: 0.0,
            ior: 1.5,
            clearcoat: 0.0,
            clearcoat_roughness: 0.05,
            normal_scale: 1.0,
        }
    }
}

impl Pbr {
    pub fn sharpness(&self) -> f32 {
        let r = self.roughness.clamp(0.02, 1.0);
        (2.0 / (r * r * r * r) - 2.0).min(4096.0)
    }

    pub fn gloss(&self) -> f32 {
        self.specular.max(0.0) * 0.24
    }

    pub fn see_through(&self) -> bool {
        self.transmission > 0.0
    }

    pub fn raster_opacity(&self) -> f32 {
        1.0 - 0.75 * self.transmission.clamp(0.0, 1.0)
    }
}

pub trait Material: Copy + Eq + Hash + Default + Send + Sync + 'static {
    fn shading(&self) -> Shading {
        Shading::default()
    }

    fn pbr(&self) -> Pbr {
        self.shading().pbr()
    }
}

impl Material for () {}

#[derive(Clone, Default)]
pub struct Surface<M = ()> {
    pub positions: Vec<V3>,
    pub normals: Vec<V3>,
    pub uvs: Vec<[f32; 2]>,
    pub colour: V3,
    pub texture: Option<usize>,
    pub normal_map: Option<usize>,
    pub metal_rough_map: Option<usize>,
    pub occlusion_map: Option<usize>,
    pub emission_map: Option<usize>,
    pub material: M,
    pub group: usize,
}

#[derive(Clone, Default)]
pub struct Lines<M = ()> {
    pub segments: Vec<[V3; 2]>,
    pub colour: V3,
    pub material: M,
    pub group: usize,
    pub width: f32,
}

#[derive(Clone, Default)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

pub struct Scene<M = ()> {
    pub id: u64,
    pub surfaces: Vec<Surface<M>>,
    pub lines: Vec<Lines<M>>,
    pub images: Vec<Image>,
    /// Explicit GPU overrides keyed by legacy image index. Keep level zero equal
    /// to the legacy image and use a fresh scene id when changing either payload.
    pub explicit_images: std::collections::BTreeMap<usize, ExplicitImage>,
    pub centre: V3,
    pub radius: f32,
}

impl<M> Default for Scene<M> {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Scene {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            surfaces: Vec::new(),
            lines: Vec::new(),
            images: Vec::new(),
            explicit_images: Default::default(),
            centre: [0.0; 3],
            radius: 1.0,
        }
    }
}

impl<M> Scene<M> {
    pub fn uses_uv_transform(&self) -> bool where M: Material {
        self.surfaces.iter().any(|s| s.material.pbr().uv_transform != [1.,1.,0.,0.])
    }
    pub fn uses_extra_maps(&self) -> bool {
        self.surfaces.iter().any(|s| s.occlusion_map.is_some() || s.emission_map.is_some())
    }

    pub fn uses_material_alpha(&self) -> bool where M: Material {
        self.surfaces.iter().any(|s| s.material.pbr().alpha != AlphaMode::Opaque)
    }

    pub fn validate_explicit_images(&self) -> Result<(), &'static str> where M: Material {
        for surface in &self.surfaces {
            let p = surface.material.pbr();
            if p.uv_transform.iter().any(|v| !v.is_finite()) || p.uv_transform[0] <= 0. || p.uv_transform[1] <= 0. {
                return Err("UV transform requires finite values and positive scale");
            }
            if !p.occlusion_strength.is_finite() || !(0.0..=1.0).contains(&p.occlusion_strength) {
                return Err("occlusion strength must be finite and in 0..1");
            }
            if [surface.occlusion_map, surface.emission_map].into_iter().flatten().any(|i| i >= self.images.len()) {
                return Err("occlusion/emission image index is missing");
            }
            if !p.base_alpha.is_finite() || !(0.0..=1.0).contains(&p.base_alpha) {
                return Err("material alpha factor must be finite and in 0..1");
            }
            if let AlphaMode::Mask { cutoff } = p.alpha {
                if !cutoff.is_finite() || !(0.0..=1.0).contains(&cutoff) {
                    return Err("material alpha cutoff must be finite and in 0..1");
                }
            }
        }
        for (&index, explicit) in &self.explicit_images {
            let base = self.images.get(index).ok_or("explicit image index is missing")?;
            let level = &explicit.levels()[0];
            if base.width != level.width || base.height != level.height || base.rgba != level.rgba {
                return Err("explicit image base differs from scene image");
            }
        }
        for surface in &self.surfaces {
            let maps = [surface.texture, surface.normal_map, surface.metal_rough_map, surface.occlusion_map, surface.emission_map];
            let explicit = maps.map(|i| i.and_then(|i| self.explicit_images.get(&i)));
            if explicit.iter().any(Option::is_some) {
                if maps.iter().zip(&explicit).any(|(map, image)| map.is_some() && image.is_none()) {
                    return Err("material mixes explicit and legacy images");
                }
                explicit_image::explicit_material_lod_with_extras(explicit)?;
            }
        }
        Ok(())
    }

    pub fn triangles(&self) -> usize {
        self.surfaces.iter().map(|s| s.positions.len() / 3).sum()
    }

    pub fn segments(&self) -> usize {
        self.lines.iter().map(|l| l.segments.len()).sum()
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct Look {
    pub placement: Placement,
    pub opacity: f32,
    pub selected: bool,
}

impl Default for Look {
    fn default() -> Self {
        Look { placement: IDENTITY, opacity: 1.0, selected: false }
    }
}

impl Look {
    pub const HIDDEN: Look = Look { placement: IDENTITY, opacity: 0.0, selected: false };

    pub fn visible(&self) -> bool {
        self.opacity > 0.0
    }

    pub fn place(&self, p: V3) -> V3 {
        let m = &self.placement;
        [
            m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
            m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
            m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
        ]
    }

    pub fn turn(&self, v: V3) -> V3 {
        let m = &self.placement;
        [
            m[0] * v[0] + m[4] * v[1] + m[8] * v[2],
            m[1] * v[0] + m[5] * v[1] + m[9] * v[2],
            m[2] * v[0] + m[6] * v[1] + m[10] * v[2],
        ]
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Light {
    pub azimuth: f32,
    pub elevation: f32,
    pub intensity: f32,
    pub colour: Color32,
    pub spread: f32,
}

impl Light {
    pub const KEY: Light = Light {
        azimuth: -0.436_6,
        elevation: 0.586_6,
        intensity: 0.78,
        colour: Color32::WHITE,
        spread: 0.05,
    };
    pub const FILL: Light = Light {
        azimuth: 0.602_2,
        elevation: -0.252_0,
        intensity: 0.25,
        colour: Color32::WHITE,
        spread: 0.3,
    };

    pub fn radiance(&self) -> V3 {
        rgb(self.colour).map(|c| to_linear(c) * self.intensity.max(0.0))
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Lighting {
    pub key: Light,
    pub fill: Light,
    pub ambient: f32,
    pub sky: Color32,
    pub ground: Color32,
    pub exposure: f32,
    pub follow: bool,
}

impl Default for Lighting {
    fn default() -> Self {
        Lighting::STUDIO
    }
}

impl Lighting {
    pub const STUDIO: Lighting = Lighting {
        key: Light::KEY,
        fill: Light::FILL,
        ambient: 0.32,
        sky: Color32::WHITE,
        ground: Color32::WHITE,
        exposure: 1.0,
        follow: true,
    };

    pub const OVERCAST: Lighting = Lighting {
        key: Light {
            azimuth: 0.7,
            elevation: 1.2,
            intensity: 0.3,
            colour: Color32::from_rgb(235, 238, 245),
            spread: 0.5,
        },
        fill: Light {
            azimuth: -2.4,
            elevation: 0.5,
            intensity: 0.1,
            colour: Color32::from_rgb(225, 230, 240),
            spread: 0.5,
        },
        ambient: 0.8,
        sky: Color32::from_rgb(228, 232, 240),
        ground: Color32::from_rgb(150, 146, 138),
        exposure: 1.0,
        follow: false,
    };

    pub fn outdoor(hour: f32) -> Lighting {
        let d = sun_direction(hour.rem_euclid(24.0));
        let elevation = d[2].asin().to_degrees();
        let day = smoothstep(-6.0, 4.0, elevation);
        let high = smoothstep(0.0, 30.0, elevation);
        let sun = mix(Color32::from_rgb(255, 128, 56), Color32::from_rgb(255, 246, 232), high);
        let blue = Color32::from_rgb(140, 180, 255);
        let sky = mix(
            Color32::from_rgb(36, 46, 84),
            mix(Color32::from_rgb(240, 160, 130), blue, high),
            day,
        );
        let sun_strength = 1.15 * smoothstep(-1.0, 10.0, elevation);
        let moon_strength = 0.14 * (1.0 - day);
        let key = if sun_strength >= moon_strength {
            toward(d, sun_strength, sun, 0.01)
        } else {
            let moon = norm([-d[0], -d[1], 0.6]);
            toward(moon, moon_strength, Color32::from_rgb(170, 190, 255), 0.02)
        };
        let bounce = norm([-d[0], -d[1], 0.4]);
        Lighting {
            key,
            fill: toward(bounce, 0.2 * day, blue, 0.5),
            ambient: 0.1 + 0.32 * day,
            sky,
            ground: mix(Color32::from_rgb(40, 40, 46), Color32::from_rgb(130, 122, 104), day),
            exposure: 1.0 + 1.4 * (1.0 - day),
            follow: false,
        }
    }

    pub fn direction(&self, light: &Light, vw: &View) -> V3 {
        let (sa, ca) = light.azimuth.sin_cos();
        let (se, ce) = light.elevation.sin_cos();
        if self.follow {
            let back = scale(vw.forward, -1.0);
            norm(add(add(scale(vw.right, ce * sa), scale(vw.up, se)), scale(back, ce * ca)))
        } else {
            [ce * ca, ce * sa, se]
        }
    }

    pub fn sky(&self) -> V3 {
        rgb(self.sky).map(|c| to_linear(c) * self.ambient.max(0.0))
    }

    pub fn ground(&self) -> V3 {
        rgb(self.ground).map(|c| to_linear(c) * self.ambient.max(0.0))
    }

    pub fn environment(&self, n: V3) -> V3 {
        let (sky, ground) = (self.sky(), self.ground());
        let t = (n[2] * 0.5 + 0.5).clamp(0.0, 1.0);
        [0, 1, 2].map(|k| ground[k] + (sky[k] - ground[k]) * t)
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Preset {
    #[default]
    Studio,
    Overcast,
    Outdoor {
        hour: f32,
    },
}

const LATITUDE: f32 = 52.0;
const DECLINATION: f32 = 12.0;

impl Preset {
    pub fn name(&self) -> &'static str {
        match self {
            Preset::Studio => "indoor studio",
            Preset::Overcast => "outdoor overcast",
            Preset::Outdoor { .. } => "outdoor sun",
        }
    }

    pub fn lighting(&self) -> Lighting {
        match *self {
            Preset::Studio => Lighting::STUDIO,
            Preset::Overcast => Lighting::OVERCAST,
            Preset::Outdoor { hour } => Lighting::outdoor(hour),
        }
    }
}

fn smoothstep(low: f32, high: f32, x: f32) -> f32 {
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

pub fn sun_direction(hour: f32) -> V3 {
    let (lat, dec) = (LATITUDE.to_radians(), DECLINATION.to_radians());
    let angle = ((hour - 12.0) * 15.0).to_radians();
    [
        -dec.cos() * angle.sin(),
        dec.sin() * lat.cos() - dec.cos() * angle.cos() * lat.sin(),
        dec.sin() * lat.sin() + dec.cos() * angle.cos() * lat.cos(),
    ]
}

fn toward(d: V3, intensity: f32, colour: Color32, spread: f32) -> Light {
    Light {
        azimuth: d[1].atan2(d[0]),
        elevation: d[2].clamp(-1.0, 1.0).asin(),
        intensity,
        colour,
        spread,
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Trace {
    pub samples: u32,
    pub bounces: u32,
    pub per_frame: u32,
}

impl Default for Trace {
    fn default() -> Self {
        Trace { samples: 256, bounces: 4, per_frame: 4 }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Painted {
    pub samples: u32,
    pub target: u32,
    pub hardware: bool,
}

impl Painted {
    pub fn traced(&self) -> bool {
        self.samples > 0
    }

    pub fn settled(&self) -> bool {
        self.target > 0 && self.samples >= self.target
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum RenderMode {
    #[default]
    Shaded,
    Wireframe,
    Triangles,
    Normals,
    Facets,
    Depth,
    Uv,
    Groups,
    Batches,
    Backfaces,
    Overdraw,
    Traversal,
}

impl RenderMode {
    pub const ALL: [RenderMode; 12] = [
        RenderMode::Shaded,
        RenderMode::Wireframe,
        RenderMode::Triangles,
        RenderMode::Normals,
        RenderMode::Facets,
        RenderMode::Depth,
        RenderMode::Uv,
        RenderMode::Groups,
        RenderMode::Batches,
        RenderMode::Backfaces,
        RenderMode::Overdraw,
        RenderMode::Traversal,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            RenderMode::Shaded => "shaded",
            RenderMode::Wireframe => "wireframe",
            RenderMode::Triangles => "triangles",
            RenderMode::Normals => "normals",
            RenderMode::Facets => "facets",
            RenderMode::Depth => "depth",
            RenderMode::Uv => "uv",
            RenderMode::Groups => "groups",
            RenderMode::Batches => "batches",
            RenderMode::Backfaces => "backfaces",
            RenderMode::Overdraw => "overdraw",
            RenderMode::Traversal => "traversal",
        }
    }

    fn capped(&self) -> bool {
        matches!(
            self,
            RenderMode::Shaded | RenderMode::Triangles | RenderMode::Groups | RenderMode::Batches
        )
    }
}

pub fn palette(index: usize) -> V3 {
    let h = (index as f32 * 0.618_034 + 0.12).fract() * 6.0;
    let (s, v) = (0.55, 0.85);
    let f = h.fract();
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let c = match h as u32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    };
    c.map(to_linear)
}

pub(super) type BatchKey<M> = (usize, M, [Option<usize>; 5]);

pub(super) fn batch_key<M: Material>(scene: &Scene<M>, s: &Surface<M>) -> BatchKey<M> {
    let image = |i: Option<usize>| i.filter(|i| *i < scene.images.len());
    (s.group, s.material, [image(s.texture), image(s.normal_map), image(s.metal_rough_map), image(s.occlusion_map), image(s.emission_map)])
}

pub(super) fn batches<M: Material>(scene: &Scene<M>) -> (Vec<Option<usize>>, usize) {
    let mut keys: Vec<BatchKey<M>> = Vec::new();
    let ids = scene
        .surfaces
        .iter()
        .map(|s| {
            if s.positions.len() < 3 {
                return None;
            }
            let key = batch_key(scene, s);
            Some(keys.iter().position(|k| *k == key).unwrap_or_else(|| {
                keys.push(key);
                keys.len() - 1
            }))
        })
        .collect();
    (ids, keys.len())
}

pub(super) type Aabb = (V3, V3);

pub(super) const EMPTY: Aabb = ([f32::MAX; 3], [f32::MIN; 3]);

pub(super) fn grow(aabb: &mut Aabb, p: V3) {
    aabb.0 = std::array::from_fn(|k| aabb.0[k].min(p[k]));
    aabb.1 = std::array::from_fn(|k| aabb.1[k].max(p[k]));
}

pub(super) fn placed(aabb: &Aabb, look: &Look) -> [V3; 8] {
    let (lo, hi) = aabb;
    std::array::from_fn(|k| {
        look.place([
            if k & 1 == 0 { lo[0] } else { hi[0] },
            if k & 2 == 0 { lo[1] } else { hi[1] },
            if k & 4 == 0 { lo[2] } else { hi[2] },
        ])
    })
}

pub(super) fn depth_range(corners: &[[V3; 8]], vw: &View, ortho: bool) -> (f32, f32) {
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for c in corners.iter().flatten() {
        let depth = dot(sub(*c, vw.eye), vw.forward);
        lo = lo.min(depth);
        hi = hi.max(depth);
    }
    if hi < lo || (!ortho && hi <= 0.0) {
        return (vw.near, vw.far);
    }
    let pad = (hi - lo).max(vw.dist * 1.0e-3) * 0.01;
    let (near, far) = (lo - pad, hi + pad);
    if ortho { (near, far) } else { (near.max(far * 1.0e-3), far) }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Style {
    pub background: Color32,
    pub lighting: Lighting,
    pub cap: Color32,
    pub selected: Color32,
    pub selected_tint: f32,
}

impl Default for Style {
    fn default() -> Self {
        Style::BENCH
    }
}

impl Style {
    pub const BENCH: Style = Style {
        background: BACKGROUND,
        lighting: Lighting::STUDIO,
        cap: Color32::from_rgb(237, 140, 46),
        selected: Color32::from_rgb(245, 166, 59),
        selected_tint: 0.35,
    };
}

pub fn rgb(c: Color32) -> V3 {
    [c.r(), c.g(), c.b()].map(|v| v as f32 / 255.0)
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn norm(a: V3) -> V3 {
    scale(a, 1.0 / dot(a, a).sqrt().max(1.0e-12))
}

pub fn hit_triangle(origin: V3, direction: V3, [a, b, c]: [V3; 3]) -> Option<f32> {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let p = cross(direction, e2);
    let det = dot(e1, p);
    if det.abs() < 1.0e-12 {
        return None;
    }
    let s = sub(origin, a);
    let u = dot(s, p) / det;
    let q = cross(s, e1);
    let v = dot(direction, q) / det;
    let t = dot(e2, q) / det;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 0.0).then_some(t)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub shift: V3,
    pub ortho: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { yaw: 0.6, pitch: 0.55, zoom: 1.0, shift: [0.0; 3], ortho: true }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cut {
    pub normal: V3,
    pub offset: f32,
}

impl Cut {
    pub fn keeps(&self, p: V3) -> bool {
        dot(p, self.normal) <= self.offset
    }
}

pub struct View {
    pub dist: f32,
    pub focal: f32,
    pub eye: V3,
    pub target: V3,
    pub right: V3,
    pub up: V3,
    pub forward: V3,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

pub fn view<M>(scene: &Scene<M>, cam: &Camera, size: Vec2) -> View {
    let (cy, sy, cp, sp) = (cam.yaw.cos(), cam.yaw.sin(), cam.pitch.cos(), cam.pitch.sin());
    let eye_dir = [sy * cp, -cy * cp, sp];
    let dist = scene.radius * DISTANCE;
    let forward = norm(scale(eye_dir, -1.0));
    let right = norm(cross(forward, [0.0, 0.0, 1.0]));
    let right = if dot(right, right) < 0.5 { [1.0, 0.0, 0.0] } else { right };
    let up = cross(right, forward);
    let focal = size.x.min(size.y) * 1.25 * cam.zoom;
    let target = add(scene.centre, cam.shift);
    let eye = add(target, scale(eye_dir, dist));
    View {
        dist,
        focal,
        eye,
        target,
        right,
        up,
        forward,
        fov_y: 2.0 * (size.y * 0.5 / focal).atan(),
        near: dist * 0.02,
        far: dist * 10.0,
    }
}

impl Camera {
    pub fn panned<M>(self, scene: &Scene<M>, size: Vec2, delta: Vec2) -> Camera {
        let f = view(scene, &self, size);
        let per_pixel = f.dist / f.focal;
        let moved = add(scale(f.right, -delta.x * per_pixel), scale(f.up, delta.y * per_pixel));
        Camera { shift: add(self.shift, moved), ..self }
    }

    pub fn zoomed<M>(
        self,
        scene: &Scene<M>,
        rect: Rect,
        at: Pos2,
        factor: f32,
        anchor: Option<V3>,
    ) -> Camera {
        let zoom = (self.zoom * factor).clamp(0.1, 400.0);
        let before = view(scene, &self, rect.size());
        let after = Camera { zoom, ..self };
        let focal_after = view(scene, &after, rect.size()).focal;
        let depth = match (self.ortho, anchor) {
            (false, Some(p)) => dot(sub(p, before.eye), before.forward).max(before.dist * 0.02),
            _ => before.dist,
        };
        let change = depth / before.focal - depth / focal_after;
        let (dx, dy) = (at.x - rect.center().x, at.y - rect.center().y);
        let moved = add(scale(before.right, dx * change), scale(before.up, -dy * change));
        Camera { shift: add(self.shift, moved), ..after }
    }

    pub fn orbited<M>(
        self,
        scene: &Scene<M>,
        size: Vec2,
        yaw: f32,
        pitch: f32,
        pivot: V3,
    ) -> Camera {
        let before = view(scene, &self, size);
        let turned = Camera {
            yaw: self.yaw + yaw,
            pitch: (self.pitch + pitch).clamp(-1.5699, 1.5699),
            ..self
        };
        let after = view(scene, &turned, size);
        let offset = sub(before.target, pivot);
        let local =
            [dot(offset, before.right), dot(offset, before.up), dot(offset, before.forward)];
        let rotated = add(
            add(scale(after.right, local[0]), scale(after.up, local[1])),
            scale(after.forward, local[2]),
        );
        let target = add(pivot, rotated);
        Camera { shift: add(turned.shift, sub(target, after.target)), ..turned }
    }

    pub fn framed<M>(self, scene: &Scene<M>, centre: V3, span: f32) -> Camera {
        let dist = scene.radius * DISTANCE;
        Camera {
            shift: sub(centre, scene.centre),
            zoom: dist / (1.25 * span.max(1.0e-6) * 1.15),
            ..self
        }
    }
}

pub struct Projector {
    pub ortho: Option<f32>,
    pub eye: V3,
    pub right: V3,
    pub up: V3,
    pub forward: V3,
    pub focal: f32,
    pub centre: Pos2,
}

impl Projector {
    pub fn new<M>(scene: &Scene<M>, cam: &Camera, rect: Rect) -> Projector {
        let vw = view(scene, cam, rect.size());
        Projector {
            ortho: cam.ortho.then_some(vw.dist),
            eye: vw.eye,
            right: vw.right,
            up: vw.up,
            forward: vw.forward,
            focal: vw.focal,
            centre: rect.center(),
        }
    }

    pub fn project(&self, p: V3) -> Option<Pos2> {
        let d = sub(p, self.eye);
        let z = dot(d, self.forward);
        if let Some(dist) = self.ortho {
            return Some(Pos2::new(
                self.centre.x + dot(d, self.right) / dist * self.focal,
                self.centre.y - dot(d, self.up) / dist * self.focal,
            ));
        }
        (z > 1.0e-6).then(|| {
            Pos2::new(
                self.centre.x + dot(d, self.right) / z * self.focal,
                self.centre.y - dot(d, self.up) / z * self.focal,
            )
        })
    }

    pub fn ray(&self, at: Pos2) -> (V3, V3) {
        let (dx, dy) = ((at.x - self.centre.x) / self.focal, (at.y - self.centre.y) / self.focal);
        if let Some(dist) = self.ortho {
            let origin =
                add(add(self.eye, scale(self.right, dx * dist)), scale(self.up, -dy * dist));
            return (origin, self.forward);
        }
        let direction = norm(add(add(self.forward, scale(self.right, dx)), scale(self.up, -dy)));
        (self.eye, direction)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Hit {
    pub surface: usize,
    pub point: V3,
    pub distance: f32,
}

#[derive(Clone, Copy, Default)]
pub struct Orbit {
    pivot: Option<V3>,
}

#[derive(Clone)]
pub struct Viewer<M: Material = ()> {
    scene: Arc<Scene<M>>,
    camera: Camera,
    looks: Arc<[Look]>,
    cut: Option<Cut>,
    style: Style,
    trace: Option<Trace>,
    environment: Option<EnvironmentLight>,
    mode: RenderMode,
    edges: bool,
}

impl<M: Material> Viewer<M> {
    pub fn new(scene: Arc<Scene<M>>, camera: Camera) -> Viewer<M> {
        Viewer {
            scene,
            camera,
            looks: Arc::new([]),
            cut: None,
            style: Style::default(),
            trace: None,
            environment: None,
            mode: RenderMode::Shaded,
            edges: true,
        }
    }

    pub(super) fn ambient(&self) -> (V3, V3) {
        match &self.environment {
            Some(e) => (e.up(), e.down()),
            None => (self.style.lighting.sky(), self.style.lighting.ground()),
        }
    }

    pub(super) fn surroundings(&self, n: V3) -> V3 {
        let (up, down) = self.ambient();
        let t = (n[2] * 0.5 + 0.5).clamp(0.0, 1.0);
        [0, 1, 2].map(|k| down[k] + (up[k] - down[k]) * t)
    }

    pub fn environment(mut self, environment: Option<EnvironmentLight>) -> Viewer<M> {
        self.environment = environment;
        self
    }

    pub fn mode(mut self, mode: RenderMode) -> Viewer<M> {
        self.mode = mode;
        self
    }

    pub fn edges(mut self, on: bool) -> Viewer<M> {
        self.edges = on;
        self
    }

    pub fn lighting(mut self, lighting: Lighting) -> Viewer<M> {
        self.style.lighting = lighting;
        self
    }

    pub fn trace(mut self, trace: Option<Trace>) -> Viewer<M> {
        self.trace = trace;
        self
    }

    pub fn looks(mut self, looks: impl Into<Arc<[Look]>>) -> Viewer<M> {
        self.looks = looks.into();
        self
    }

    pub fn cut(mut self, cut: Option<Cut>) -> Viewer<M> {
        self.cut = cut;
        self
    }

    pub fn style(mut self, style: Style) -> Viewer<M> {
        self.style = style;
        self
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }

    pub fn scene(&self) -> &Scene<M> {
        &self.scene
    }

    pub fn look(&self, group: usize) -> Look {
        self.looks.get(group).copied().unwrap_or_default()
    }

    pub fn projector(&self, rect: Rect) -> Projector {
        Projector::new(&self.scene, &self.camera, rect)
    }

    pub fn view(&self, size: Vec2) -> View {
        view(&self.scene, &self.camera, size)
    }

    pub fn pick(&self, rect: Rect, at: Pos2) -> Option<Hit> {
        let (origin, direction) = self.projector(rect).ray(at);
        let along = |d: f32| add(origin, scale(direction, d));
        let kept = |p: V3| self.cut.is_none_or(|c| c.keeps(p));
        self.scene
            .surfaces
            .iter()
            .enumerate()
            .filter(|(_, s)| self.look(s.group).visible())
            .flat_map(|(index, s)| {
                let look = self.look(s.group);
                let ghost = look.opacity < 1.0;
                s.positions.chunks_exact(3).filter_map(move |t| {
                    let corners = [look.place(t[0]), look.place(t[1]), look.place(t[2])];
                    hit_triangle(origin, direction, corners)
                        .filter(|d| kept(along(*d)))
                        .map(|d| (ghost, d, index))
                })
            })
            .min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
            .map(|(_, distance, surface)| Hit { surface, point: along(distance), distance })
    }

    pub fn navigate(&mut self, ui: &Ui, response: &Response, orbit: &mut Orbit) -> bool {
        let before = self.camera;
        let rect = response.rect;
        let panning = ui.input(|i| i.modifiers.shift);
        if response.drag_started_by(egui::PointerButton::Primary) && !panning {
            orbit.pivot =
                response.interact_pointer_pos().and_then(|at| self.pick(rect, at)).map(|h| h.point);
        }
        if response.dragged_by(egui::PointerButton::Primary) && !panning {
            let d = response.drag_delta();
            let pivot = orbit.pivot.unwrap_or_else(|| self.view(rect.size()).target);
            self.camera =
                self.camera.orbited(&self.scene, rect.size(), -d.x * 0.01, d.y * 0.01, pivot);
        } else if response.dragged() {
            self.camera = self.camera.panned(&self.scene, rect.size(), response.drag_delta());
        }
        if response.hovered()
            && let Some(at) = response.hover_pos()
        {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (1.0 + scroll * 0.002);
            if (factor - 1.0).abs() > 1.0e-4 {
                let anchor =
                    if self.camera.ortho { None } else { self.pick(rect, at).map(|h| h.point) };
                self.camera = self.camera.zoomed(&self.scene, rect, at, factor, anchor);
            }
        }
        self.camera != before
    }

    /// Query before render_gpu to distinguish unsupported modes from no GPU.
    pub fn explicit_image_error(&self) -> Option<&'static str> {
        if let Err(error) = self.scene.validate_explicit_images() { return Some(error); }
        if self.scene.uses_uv_transform() {
            if self.mode == RenderMode::Traversal || (self.mode == RenderMode::Shaded && self.trace.is_some()) {
                return Some("UV transform does not support tracing preview");
            }
            if !gpu::installed() { return Some("UV transform requires GPU raster preview"); }
        }
        if self.scene.uses_extra_maps() {
            if self.mode == RenderMode::Traversal || (self.mode == RenderMode::Shaded && self.trace.is_some()) {
                return Some("occlusion/emission maps do not support tracing preview");
            }
            if !gpu::installed() { return Some("occlusion/emission maps require GPU raster preview"); }
        }
        if self.scene.uses_material_alpha() {
            if self.mode == RenderMode::Traversal || (self.mode == RenderMode::Shaded && self.trace.is_some()) {
                return Some("material alpha does not support tracing preview");
            }
            if !gpu::installed() { return Some("material alpha requires GPU raster preview"); }
        }
        if !self.scene.explicit_images.is_empty() &&
            (self.mode == RenderMode::Traversal || (self.mode == RenderMode::Shaded && self.trace.is_some())) {
            return Some("explicit mip images do not support tracing preview");
        }
        if !self.scene.explicit_images.is_empty() && !gpu::installed() {
            return Some("explicit mip images require GPU raster preview");
        }
        None
    }

    pub fn paint(&self, ui: &Ui, rect: Rect) -> Painted {
        if let Some(error) = self.explicit_image_error() {
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, error,
                egui::FontId::proportional(14.0), egui::Color32::RED);
            return Painted::default();
        }
        gpu::paint(ui, rect, self)
    }

    pub fn render_gpu(&self, width: usize, height: usize) -> Option<ColorImage> {
        if self.explicit_image_error().is_some() { return None; }
        gpu::render_gpu(self, width, height)
    }

    pub fn render_soft(&self, width: usize, height: usize) -> ColorImage {
        assert!(!self.scene.uses_uv_transform(), "UV transform requires GPU raster preview");
        assert!(!self.scene.uses_material_alpha(), "material alpha requires GPU raster preview");
        assert!(self.scene.explicit_images.is_empty(), "explicit mip images require GPU raster preview");
        assert!(!self.scene.uses_extra_maps(), "occlusion/emission maps require GPU raster preview");
        soft::render(self, width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Scene {
        Scene { centre: [1.0, 2.0, 3.0], radius: 10.0, ..Default::default() }
    }

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(800.0, 600.0))
    }

    fn cameras() -> [Camera; 2] {
        [false, true].map(|ortho| Camera { ortho, ..Camera::default() })
    }

    #[test]
    fn picking_rays_land_where_points_project() {
        for cam in cameras() {
            let projector = Projector::new(&scene(), &cam, rect());
            let at = Pos2::new(250.0, 410.0);
            let (origin, direction) = projector.ray(at);
            let back = projector.project(add(origin, scale(direction, 23.0))).expect("in front");
            assert!(back.distance(at) < 1.0e-2, "ortho {}: {back:?}", cam.ortho);
        }
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        for cam in cameras() {
            let point = [4.0, -3.0, 6.0];
            let at = Projector::new(&scene(), &cam, rect()).project(point).expect("in front");
            for factor in [1.8, 0.6] {
                let zoomed = cam.zoomed(&scene(), rect(), at, factor, Some(point));
                let after =
                    Projector::new(&scene(), &zoomed, rect()).project(point).expect("in front");
                assert!(after.distance(at) < 1.0e-2, "ortho {}: {at:?} -> {after:?}", cam.ortho);
                assert!((zoomed.zoom - factor).abs() < 1.0e-6);
            }
        }
    }

    #[test]
    fn orbit_keeps_the_pivot_in_place() {
        for cam in cameras() {
            let pivot = [6.0, 1.0, -2.0];
            let at = Projector::new(&scene(), &cam, rect()).project(pivot).expect("in front");
            let turned = cam.orbited(&scene(), rect().size(), 0.4, -0.3, pivot);
            let after = Projector::new(&scene(), &turned, rect()).project(pivot).expect("in front");
            assert!(after.distance(at) < 1.0e-2, "ortho {}: {at:?} -> {after:?}", cam.ortho);
            assert!((turned.yaw - cam.yaw - 0.4).abs() < 1.0e-6);
        }
    }

    #[test]
    fn pan_moves_the_model_with_the_pointer() {
        for cam in cameras() {
            let point = scene().centre;
            let at = Projector::new(&scene(), &cam, rect()).project(point).expect("in front");
            let delta = Vec2::new(30.0, -12.0);
            let panned = cam.panned(&scene(), rect().size(), delta);
            let after = Projector::new(&scene(), &panned, rect()).project(point).expect("in front");
            assert!(after.distance(at + delta) < 1.0e-2, "ortho {}: {after:?}", cam.ortho);
        }
    }

    #[test]
    fn framing_fits_the_span_across_the_short_side() {
        for cam in cameras() {
            let centre = [5.0, -4.0, 0.0];
            let framed = cam.framed(&scene(), centre, 8.0);
            let projector = Projector::new(&scene(), &framed, rect());
            let a = projector.project(add(centre, scale(projector.right, -4.0))).unwrap();
            let b = projector.project(add(centre, scale(projector.right, 4.0))).unwrap();
            assert!(projector.project(centre).unwrap().distance(rect().center()) < 1.0e-2);
            if cam.ortho {
                assert!((b.x - a.x - 600.0 / 1.15).abs() < 1.0e-2, "{}", b.x - a.x);
            }
        }
    }

    #[test]
    fn the_studio_lights_sit_where_the_old_fixed_lights_did() {
        let vw = view(&scene(), &Camera::default(), rect().size());
        let l = Lighting::STUDIO;
        let back = scale(vw.forward, -1.0);
        let old = |r: f32, u: f32, b: f32| {
            norm(add(add(scale(vw.right, r), scale(vw.up, u)), scale(back, b)))
        };
        for (light, want) in [(&l.key, old(-0.35, 0.55, 0.75)), (&l.fill, old(0.55, -0.25, 0.8))] {
            let got = l.direction(light, &vw);
            assert!(dot(got, want) > 0.99999, "{got:?} vs {want:?}");
        }
        let world = Lighting { follow: false, ..l };
        let up = Light { elevation: std::f32::consts::FRAC_PI_2, ..Light::KEY };
        assert!(world.direction(&up, &vw)[2] > 0.9999);
    }

    #[test]
    fn a_cut_keeps_one_side() {
        let cut = Cut { normal: [0.0, -1.0, 0.0], offset: -2.0 };
        assert!(cut.keeps([0.0, 3.0, 0.0]) && !cut.keeps([0.0, 1.0, 0.0]));
    }

    fn square(group: usize, z: f32) -> Surface {
        let (a, b, c, d) = ([-1.0, -1.0, z], [1.0, -1.0, z], [1.0, 1.0, z], [-1.0, 1.0, z]);
        Surface {
            positions: vec![a, b, c, a, c, d],
            normals: vec![[0.0, 0.0, 1.0]; 6],
            colour: [0.5; 3],
            group,
            ..Default::default()
        }
    }

    #[test]
    fn picking_prefers_solid_parts_over_ghosts_in_front() {
        let scene = Scene {
            surfaces: vec![square(0, 1.0), square(1, 0.0)],
            radius: 2.0,
            ..Default::default()
        };
        let top = Camera { pitch: 1.5699, yaw: 0.0, ..Camera::default() };
        let viewer = Viewer::new(Arc::new(scene), top);
        assert_eq!(viewer.pick(rect(), rect().center()).map(|h| h.surface), Some(0));
        let ghost = Look { opacity: 0.4, ..Look::default() };
        let viewer = viewer.looks(vec![ghost, Look::default()]);
        let hit = viewer.pick(rect(), rect().center()).unwrap();
        assert_eq!(hit.surface, 1);
        assert!(hit.point[2].abs() < 1.0e-4, "{hit:?}");
        let viewer = viewer.looks(vec![Look::HIDDEN, Look::HIDDEN]);
        assert!(viewer.pick(rect(), rect().center()).is_none());
    }

    #[test]
    fn soft_render_draws_a_visible_surface_and_skips_a_hidden_one() {
        let scene =
            Arc::new(Scene { surfaces: vec![square(0, 0.0)], radius: 2.0, ..Default::default() });
        let top = Camera { pitch: 1.5699, yaw: 0.0, ..Camera::default() };
        let lit = Viewer::new(scene.clone(), top).render_soft(64, 48);
        assert_ne!(lit.pixels[24 * 64 + 32], BACKGROUND);
        assert_eq!(lit.pixels[0], BACKGROUND);
        let hidden = Viewer::new(scene, top).looks(vec![Look::HIDDEN]).render_soft(64, 48);
        assert!(hidden.pixels.iter().all(|p| *p == BACKGROUND));
    }
}
