use super::*;
use std::io::Read;

fn requested_render_state() -> egui_wgpu::RenderState {
    let (backends, expected) = match std::env::var("GCAD_RENDER_BACKEND").as_deref() {
        Ok("vulkan") => (wgpu::Backends::VULKAN, wgpu::Backend::Vulkan),
        Ok("dx12") | Err(std::env::VarError::NotPresent) => {
            (wgpu::Backends::DX12, wgpu::Backend::Dx12)
        }
        other => panic!("unsupported GCAD_RENDER_BACKEND: {other:?}"),
    };
    let egui_wgpu::WgpuSetup::CreateNew(mut setup) = egui_kittest::wgpu::default_wgpu_setup()
    else {
        panic!("expected a new headless renderer");
    };
    setup.instance_descriptor.backends = backends;
    let state = egui_kittest::wgpu::create_render_state(
        egui_wgpu::WgpuSetup::CreateNew(setup),
        egui_wgpu::RendererOptions::PREDICTABLE,
    );
    assert_eq!(
        state.adapter.get_info().backend,
        expected,
        "backend fallback is forbidden"
    );
    println!("Verified requested backend: {expected:?}");
    state
}

fn fixture_u32(reader: &mut impl Read) -> u32 {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn gcad_explicit_real_atlas_matches_separate_chart_references() {
    let path = std::env::var("GCAD_ATLAS_FIXTURE")
        .expect("set GCAD_ATLAS_FIXTURE to the gcad-generated binary fixture");
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .unwrap()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= 1024 * 1024);
    let mut reader = std::io::Cursor::new(&bytes);
    let mut magic = [0; 8];
    reader.read_exact(&mut magic).unwrap();
    assert_eq!(&magic, b"GCADAT01");
    assert_eq!(fixture_u32(&mut reader), 6);
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    for _ in 0..6 {
        let v: [f32; 8] = std::array::from_fn(|_| f32::from_bits(fixture_u32(&mut reader)));
        assert!(v.iter().all(|v| v.is_finite()));
        positions.push([v[0], v[1], v[2]]);
        normals.push([v[3], v[4], v[5]]);
        uvs.push([v[6], v[7]]);
    }
    let mut atlas_scene: Scene = Scene::default();
    atlas_scene.radius = 1.;
    for slot in 0..3 {
        assert_eq!(fixture_u32(&mut reader), 4);
        let mut levels = Vec::new();
        for level in 0..4 {
            let w = fixture_u32(&mut reader) as usize;
            let h = fixture_u32(&mut reader) as usize;
            assert_eq!((w, h), (128 >> level, 64 >> level));
            let mut rgba = vec![0; w * h * 4];
            reader.read_exact(&mut rgba).unwrap();
            levels.push(Image {
                width: w,
                height: h,
                rgba,
            });
        }
        atlas_scene.images.push(levels[0].clone());
        atlas_scene.explicit_images.insert(
            slot,
            ExplicitImage::new(
                levels,
                if slot == 0 {
                    ImageEncoding::Srgb
                } else {
                    ImageEncoding::Linear
                },
            )
            .unwrap(),
        );
    }
    assert_eq!(reader.position() as usize, bytes.len());
    let mut reference: Scene = Scene::default();
    reference.radius = 1.;
    for chart in 0..2 {
        let surface = Surface {
            positions: positions[chart * 3..chart * 3 + 3].to_vec(),
            normals: normals[chart * 3..chart * 3 + 3].to_vec(),
            uvs: uvs[chart * 3..chart * 3 + 3].to_vec(),
            colour: [1.; 3],
            texture: Some(0),
            normal_map: Some(1),
            metal_rough_map: Some(2),
            ..Default::default()
        };
        atlas_scene.surfaces.push(surface.clone());
        let base = reference.images.len();
        reference.images.extend([
            image(
                1,
                if chart == 0 {
                    [137, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                },
            ),
            image(1, [128, 191, 255, 255]),
            image(1, [255, 64, 192, 255]),
        ]);
        reference.surfaces.push(Surface {
            texture: Some(base),
            normal_map: Some(base + 1),
            metal_rough_map: Some(base + 2),
            ..surface
        });
    }
    let state = requested_render_state();
    gpu::install(&state);
    let atlas = Viewer::new(Arc::new(atlas_scene), Camera::default()).edges(false);
    let reference = Viewer::new(Arc::new(reference), Camera::default()).edges(false);
    let mut compared = 0;
    let mut maximum = 0;
    for size in [128, 32, 8] {
        let actual = atlas
            .render_gpu(size, size)
            .expect("atlas render requires GPU");
        let expected = reference
            .render_gpu(size, size)
            .expect("reference render requires GPU");
        let mut material_pixels = 0;
        let mut worst = (0, 0, [0; 4], [0; 4]);
        for (index, (a, b)) in actual.pixels.iter().zip(&expected.pixels).enumerate() {
            if *b != expected.pixels[0] {
                material_pixels += 1;
            }
            for (x, y) in a.to_array().into_iter().zip(b.to_array()) {
                maximum = maximum.max(x.abs_diff(y));
                if x.abs_diff(y) > worst.0 {
                    worst = (x.abs_diff(y), index, a.to_array(), b.to_array());
                }
            }
            compared += 1;
        }
        assert!(material_pixels > 0, "empty atlas render at size {size}");
        println!(
            "Atlas diagnostic size {size}: worst {worst:?}, material pixels {material_pixels}"
        );
    }
    assert!(
        maximum <= 2,
        "gcad atlas differs from independent chart reference: {maximum}"
    );
    println!(
        "Candidate gcad atlas: {compared} pixels across 128/32/8 sizes, maximum byte error {maximum}; backend {:?}",
        state.adapter.get_info().backend
    );
}

fn image(size: usize, colour: [u8; 4]) -> Image {
    Image {
        width: size,
        height: size,
        rgba: colour.repeat(size * size),
    }
}
fn scene(explicit: bool) -> Scene {
    let mut scene = Scene::default();
    scene.radius = 1.;
    scene.surfaces.push(Surface {
        positions: vec![[-1., -1., 0.], [1., -1., 0.], [0., 1., 0.]],
        normals: vec![[0., 0., 1.]; 3],
        uvs: vec![[0., 0.], [1., 0.], [0.5, 1.]],
        colour: [1.; 3],
        texture: Some(0),
        normal_map: Some(1),
        metal_rough_map: Some(2),
        ..Default::default()
    });
    for (i, colour) in [
        [137, 64, 200, 255],
        [128, 128, 255, 255],
        [255, 100, 50, 255],
    ]
    .into_iter()
    .enumerate()
    {
        scene.images.push(image(16, colour));
        if explicit {
            scene.explicit_images.insert(
                i,
                ExplicitImage::new(
                    vec![image(16, colour), image(8, colour)],
                    if i == 0 {
                        ImageEncoding::Srgb
                    } else {
                        ImageEncoding::Linear
                    },
                )
                .unwrap(),
            );
        }
    }
    scene
}

#[test]
fn gcad_explicit_scene_rejects_stale_mixed_and_wrong_encoding_images() {
    let mut s = scene(true);
    assert_eq!(s.validate_explicit_images(), Ok(()));
    s.images[0].rgba[0] = 0;
    assert!(
        s.validate_explicit_images()
            .unwrap_err()
            .contains("base differs")
    );
    s.images[0].rgba[0] = 137;
    let removed = s.explicit_images.remove(&1).unwrap();
    assert!(
        s.validate_explicit_images()
            .unwrap_err()
            .contains("mixes explicit")
    );
    s.explicit_images.insert(1, removed);
    s.explicit_images.insert(
        0,
        ExplicitImage::new(vec![s.images[0].clone()], ImageEncoding::Linear).unwrap(),
    );
    assert!(
        s.validate_explicit_images()
            .unwrap_err()
            .contains("encoding mismatch")
    );
}

#[test]
fn gcad_explicit_renderer_matches_legacy_constant_material_without_double_decode() {
    let state = requested_render_state();
    println!(
        "Candidate renderer backend: {:?}",
        state.adapter.get_info().backend
    );
    gpu::install(&state);
    let legacy = Viewer::new(Arc::new(scene(false)), Camera::default()).edges(false);
    let explicit = Viewer::new(Arc::new(scene(true)), Camera::default()).edges(false);
    assert_eq!(explicit.explicit_image_error(), None);
    let a = legacy
        .render_gpu(96, 96)
        .expect("legacy render requires GPU");
    let b = explicit
        .render_gpu(96, 96)
        .expect("explicit render requires GPU");
    assert_eq!(a.pixels.len(), b.pixels.len());
    let mut different_background = 0;
    let mut max_error = 0;
    for (pa, pb) in a.pixels.iter().zip(&b.pixels) {
        if *pa != a.pixels[0] {
            different_background += 1;
        }
        for (x, y) in pa.to_array().into_iter().zip(pb.to_array()) {
            max_error = max_error.max(x.abs_diff(y));
        }
    }
    assert!(
        different_background > 500,
        "fixture did not render enough material pixels"
    );
    assert!(
        max_error <= 2,
        "explicit path changed constant shading: {max_error}"
    );
    println!(
        "Candidate renderer: {} pixels compared, {} material pixels, maximum byte error {}",
        a.pixels.len(),
        different_background,
        max_error
    );
    // A green supplied lower level must visibly replace the purple base. This
    // prevents a legacy-only implementation from passing the equality check.
    // A base-only normal chain then forces the shared sampler back to LOD zero.
    for short_normal in [false, true] {
        let mut supplied = scene(true);
        supplied.images[0] = image(256, [137, 64, 200, 255]);
        supplied.explicit_images.insert(
            0,
            ExplicitImage::new(
                vec![supplied.images[0].clone(), image(128, [0, 255, 0, 255])],
                ImageEncoding::Srgb,
            )
            .unwrap(),
        );
        if short_normal {
            supplied.explicit_images.insert(
                1,
                ExplicitImage::new(vec![supplied.images[1].clone()], ImageEncoding::Linear)
                    .unwrap(),
            );
        }
        let rendered = Viewer::new(Arc::new(supplied), Camera::default())
            .edges(false)
            .render_gpu(96, 96)
            .expect("supplied level render requires GPU");
        let mut green = 0;
        let mut error = 0;
        for (original, actual) in a.pixels.iter().zip(&rendered.pixels) {
            let [r, g, b, _] = actual.to_array();
            if g as u16 > r as u16 + 16 && g as u16 > b as u16 + 16 {
                green += 1;
            }
            for (x, y) in original.to_array().into_iter().zip(actual.to_array()) {
                error = error.max(x.abs_diff(y));
            }
        }
        if short_normal {
            assert!(error <= 2, "shortest-chain LOD bound failed: {error}");
        } else {
            assert!(
                green > 1000,
                "renderer ignored supplied lower level: {green} green pixels"
            );
        }
        println!(
            "Candidate supplied levels: short normal {short_normal}, green pixels {green}, reference error {error}"
        );
    }
    let traced = Viewer::new(Arc::new(scene(true)), Camera::default()).mode(RenderMode::Traversal);
    assert!(traced.explicit_image_error().unwrap().contains("tracing"));
    assert!(traced.render_gpu(16, 16).is_none());
}
