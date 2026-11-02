struct Frame {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    key: vec4<f32>,
    fill: vec4<f32>,
    key_radiance: vec4<f32>,
    fill_radiance: vec4<f32>,
    sky: vec4<f32>,
    ground: vec4<f32>,
    cap: vec4<f32>,
    clip: vec4<f32>,
    viewport: vec4<f32>,
    forward: vec4<f32>,
    debug: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    env: vec4<f32>,
}

struct Draw {
    model: mat4x4<f32>,
    normal: mat4x4<f32>,
    tint: vec4<f32>,
    surface: vec4<f32>,
    params: vec4<f32>,
    colour: vec4<f32>,
    emission: vec4<f32>,
    coat: vec4<f32>,
    alpha: vec4<f32>,
    extra_maps: vec4<f32>,
    uv_transform: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var<uniform> draw: Draw;
@group(2) @binding(0) var tex: texture_2d<f32>;
@group(2) @binding(1) var samp: sampler;
@group(2) @binding(2) var normal_tex: texture_2d<f32>;
@group(2) @binding(3) var metal_rough_tex: texture_2d<f32>;
@group(2) @binding(4) var occlusion_tex: texture_2d<f32>;
@group(2) @binding(5) var emission_tex: texture_2d<f32>;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c < vec3<f32>(0.04045));
}

fn to_srgb(raw: vec3<f32>) -> vec3<f32> {
    let c = clamp(raw, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c < vec3<f32>(0.0031308));
}

fn clipped(pos: vec3<f32>) -> bool {
    return frame.viewport.z > 0.5 && dot(pos, frame.clip.xyz) > frame.clip.w;
}

struct SurfaceIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) colour: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) tangent: vec4<f32>,
}

struct SurfaceOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) pos: vec3<f32>,
    @location(1) nor: vec3<f32>,
    @location(2) col: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) bary: vec3<f32>,
    @location(5) tangent: vec4<f32>,
    @location(6) @interpolate(perspective, centroid) atlas_uv: vec2<f32>,
}

const SHADED: u32 = 0u;
const WIREFRAME: u32 = 1u;
const TRIANGLES: u32 = 2u;
const NORMALS: u32 = 3u;
const FACETS: u32 = 4u;
const DEPTH: u32 = 5u;
const UV: u32 = 6u;
const GROUPS: u32 = 7u;
const BATCHES: u32 = 8u;
const BACKFACES: u32 = 9u;
const OVERDRAW: u32 = 10u;

@vertex
fn surface_vertex(v: SurfaceIn, @builtin(vertex_index) index: u32) -> SurfaceOut {
    let world = draw.model * vec4<f32>(v.position, 1.0);
    var out: SurfaceOut;
    out.clip = frame.view_proj * world;
    out.pos = world.xyz / world.w;
    out.nor = (draw.normal * vec4<f32>(v.normal, 0.0)).xyz;
    out.col = v.colour;
    out.uv = v.uv;
    out.atlas_uv = v.uv;
    out.tangent = vec4<f32>((draw.model * vec4<f32>(v.tangent.xyz, 0.0)).xyz, v.tangent.w);
    let corner = index % 3u;
    out.bary = vec3<f32>(f32(corner == 0u), f32(corner == 1u), f32(corner == 2u));
    return out;
}

fn wire(b: vec3<f32>) -> f32 {
    let dx = dpdx(b);
    let dy = dpdy(b);
    let pixels = b / max(sqrt(dx * dx + dy * dy), vec3<f32>(1.0e-8));
    let near = min(min(pixels.x, pixels.y), pixels.z);
    return 1.0 - smoothstep(0.0, frame.debug.w, near);
}

fn sharpness(roughness: f32) -> f32 {
    let r = clamp(roughness, 0.02, 1.0);
    return min(2.0 / (r * r * r * r) - 2.0, 4096.0);
}

fn facing(n: vec3<f32>, v: vec3<f32>) -> vec3<f32> {
    return select(n, -n, dot(n, v) < 0.0);
}

fn lit(albedo: vec3<f32>, normal: vec3<f32>, base: vec3<f32>, pos: vec3<f32>, rough: f32, metal: f32, occlusion: f32, emission: vec3<f32>) -> vec3<f32> {
    let key = frame.key.xyz;
    let fill = frame.fill.xyz;
    let v = normalize(frame.eye.xyz - pos);
    let n = facing(normalize(normal), v);
    let b = facing(normalize(base), v);
    let d = environment(n) * occlusion + frame.key_radiance.rgb * max(dot(n, key), 0.0)
        + frame.fill_radiance.rgb * max(dot(n, fill), 0.0);
    let h = normalize(key + v);
    let sp = draw.surface.z * 0.24 * pow(max(dot(n, h), 1.0e-6), sharpness(rough));
    let shine = vec3<f32>(1.0) + (albedo * 1.6 - vec3<f32>(1.0)) * metal;
    let coat = draw.coat.x * 0.12 * pow(max(dot(b, h), 1.0e-6), sharpness(draw.coat.y));
    let reflected = (sp * shine + vec3<f32>(coat)) * frame.key_radiance.rgb;
    return to_srgb((albedo * d + reflected + emission) * frame.key.w);
}

fn mapped_normal(in: SurfaceOut, sample: vec3<f32>) -> vec3<f32> {
    let n = normalize(in.nor);
    if draw.params.w < 0.5 || dot(in.tangent.xyz, in.tangent.xyz) < 1.0e-12 {
        return n;
    }
    let t = normalize(in.tangent.xyz - n * dot(n, in.tangent.xyz));
    let b = cross(n, t) * in.tangent.w;
    let m = sample * 2.0 - 1.0;
    let scale = draw.emission.w;
    return normalize(t * m.x * scale + b * m.y * scale + n * m.z);
}

@fragment
fn surface_fragment(in: SurfaceOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let uv = select(in.uv, in.atlas_uv, draw.coat.w > 0.5) * draw.uv_transform.xy + draw.uv_transform.zw;
    let texel = textureSample(tex, samp, uv);
    let sampled = texel.rgb;
    let bump = textureSample(normal_tex, samp, uv).rgb;
    let metal_rough = textureSample(metal_rough_tex, samp, uv).rgb;
    let ao_sample = textureSample(occlusion_tex, samp, uv).r;
    let emission_sample = textureSample(emission_tex, samp, uv).rgb;
    let occlusion = select(1.0, mix(1.0, ao_sample, draw.extra_maps.y), draw.extra_maps.x > 0.5);
    var emission = draw.emission.rgb;
    if draw.extra_maps.z > 0.5 {
        emission *= select(to_linear(emission_sample), emission_sample, draw.extra_maps.z > 1.5);
    }
    let edge = wire(in.bary);
    let facet = cross(dpdx(in.pos), dpdy(in.pos));
    if clipped(in.pos) {
        discard;
    }
    let mode = u32(frame.debug.x);
    var alpha = draw.params.x;
    let coverage = texel.a * draw.alpha.y;
    if draw.alpha.x > 1.5 {
        if coverage < draw.alpha.z { discard; }
    } else if draw.alpha.x > 0.5 {
        alpha *= coverage;
    }
    var albedo = in.col;
    if draw.params.y > 0.5 {
        albedo = select(to_linear(sampled), sampled, draw.params.y > 1.5);
    }
    albedo = mix(albedo, draw.tint.rgb, draw.tint.a);
    var rough = draw.surface.x;
    var metal = draw.surface.y;
    if draw.coat.z > 0.5 {
        rough *= metal_rough.g;
        metal *= metal_rough.b;
    }
    let shading_normal = mapped_normal(in, bump);
    switch mode {
        case WIREFRAME: {
            if edge < 0.5 {
                discard;
            }
            return vec4<f32>(to_srgb(albedo * 0.6 + vec3<f32>(0.3)), alpha);
        }
        case NORMALS: {
            return vec4<f32>(shading_normal * 0.5 + 0.5, alpha);
        }
        case DEPTH: {
            let depth = dot(in.pos - frame.eye.xyz, frame.forward.xyz);
            let t = clamp((depth - frame.debug.y) / max(frame.debug.z - frame.debug.y, 1.0e-12), 0.0, 1.0);
            return vec4<f32>(vec3<f32>(1.0 - t * 0.85), alpha);
        }
        case UV: {
            let cells = floor(in.uv * 8.0);
            let even = (i32(cells.x) + i32(cells.y)) % 2 == 0;
            let cell = vec3<f32>(fract(in.uv), 0.5) * select(0.55, 1.0, even);
            return vec4<f32>(cell, alpha);
        }
        case OVERDRAW: {
            return vec4<f32>(0.1, 0.05, 0.02, 1.0);
        }
        default: {}
    }
    let capped_mode = mode == SHADED || mode == TRIANGLES || mode == GROUPS || mode == BATCHES;
    if capped_mode && frame.viewport.z > 0.5 && draw.surface.w > 0.5 && alpha > 0.999 && !front {
        let c = normalize(frame.clip.xyz);
        let d = environment(c) + frame.key_radiance.rgb * abs(dot(c, frame.key.xyz))
            + frame.fill_radiance.rgb * abs(dot(c, frame.fill.xyz));
        return vec4<f32>(to_srgb(to_linear(frame.cap.rgb) * d * frame.key.w), 1.0);
    }
    var normal = shading_normal;
    if mode == GROUPS || mode == BATCHES {
        albedo = draw.colour.rgb;
    } else if mode == BACKFACES {
        albedo = select(vec3<f32>(0.85, 0.2, 0.15), vec3<f32>(0.2, 0.4, 0.85), front);
    } else if mode == FACETS {
        normal = facet;
    }
    var colour = lit(albedo, normal, in.nor, in.pos, rough, metal, occlusion, emission);
    if mode == TRIANGLES {
        colour = mix(colour, vec3<f32>(0.06, 0.07, 0.08), edge * 0.85);
    }
    return vec4<f32>(colour, alpha);
}

fn environment(n: vec3<f32>) -> vec3<f32> {
    return mix(frame.ground.rgb, frame.sky.rgb, clamp(n.z * 0.5 + 0.5, 0.0, 1.0));
}

struct LineIn {
    @location(0) start: vec3<f32>,
    @location(1) end: vec3<f32>,
}

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) pos: vec3<f32>,
}

@vertex
fn line_vertex(@builtin(vertex_index) index: u32, l: LineIn) -> LineOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, -1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, -1.0),
    );
    let corner = corners[index];
    let a = frame.view_proj * draw.model * vec4<f32>(l.start, 1.0);
    let b = frame.view_proj * draw.model * vec4<f32>(l.end, 1.0);
    let first = corner.x < 0.5;
    let here = select(b, a, first);
    let size = frame.viewport.xy;
    let along = (b.xy / b.w - a.xy / a.w) * size;
    let length_px = length(along);
    let dir = select(vec2<f32>(1.0, 0.0), along / length_px, length_px > 1.0e-6);
    let across = vec2<f32>(-dir.y, dir.x);
    let offset = (across * corner.y + dir * select(0.5, -0.5, first)) * draw.params.z / size;
    var out: LineOut;
    out.clip = here + vec4<f32>(offset * here.w, 0.0, 0.0);
    let world = draw.model * vec4<f32>(select(l.end, l.start, first), 1.0);
    out.pos = world.xyz / world.w;
    return out;
}

@fragment
fn line_fragment(in: LineOut) -> @location(0) vec4<f32> {
    if clipped(in.pos) {
        discard;
    }
    return vec4<f32>(draw.colour.rgb, draw.params.x);
}

@group(1) @binding(1) var sky_map: texture_2d<f32>;

struct BackdropOut {
    @builtin(position) clip: vec4<f32>,
}

@vertex
fn backdrop_vertex(@builtin(vertex_index) index: u32) -> BackdropOut {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    var out: BackdropOut;
    out.clip = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 1.0, 1.0);
    return out;
}

fn sky_texel(x: i32, y: i32, size: vec2<i32>) -> vec3<f32> {
    let wrapped = ((x % size.x) + size.x) % size.x;
    return textureLoad(sky_map, vec2<i32>(wrapped, clamp(y, 0, size.y - 1)), 0).rgb;
}

fn sky_radiance(d: vec3<f32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(sky_map));
    let tau = 6.2831853;
    let phi = atan2(d.y, d.x) - frame.env.x;
    let u = (phi / tau - floor(phi / tau)) * f32(size.x) - 0.5;
    let v = clamp(acos(clamp(d.z, -1.0, 1.0)) / 3.14159265 * f32(size.y) - 0.5, 0.0, f32(size.y - 1));
    let x0 = i32(floor(u));
    let y0 = i32(floor(v));
    let f = vec2<f32>(u - floor(u), v - floor(v));
    let top = mix(sky_texel(x0, y0, size), sky_texel(x0 + 1, y0, size), f.x);
    let bottom = mix(sky_texel(x0, y0 + 1, size), sky_texel(x0 + 1, y0 + 1, size), f.x);
    return mix(top, bottom, f.y) * frame.env.y;
}

@fragment
fn backdrop_fragment(in: BackdropOut) -> @location(0) vec4<f32> {
    let focal = frame.right.w;
    let dx = (in.clip.x - frame.viewport.x * 0.5) / focal;
    let dy = (in.clip.y - frame.viewport.y * 0.5) / focal;
    var d = frame.forward.xyz;
    if frame.up.w <= 0.0 {
        d = normalize(frame.forward.xyz + frame.right.xyz * dx - frame.up.xyz * dy);
    }
    return vec4<f32>(to_srgb(sky_radiance(d) * frame.key.w), 1.0);
}
