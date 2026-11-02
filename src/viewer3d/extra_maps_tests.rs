use super::*;

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
struct ExtraMaterial([u32; 4]);
impl Material for ExtraMaterial {
    fn pbr(&self) -> Pbr {
        Pbr {
            emission: std::array::from_fn(|i| f32::from_bits(self.0[i])),
            occlusion_strength: f32::from_bits(self.0[3]),
            ..Default::default()
        }
    }
}
fn material(emission: [f32; 3], strength: f32) -> ExtraMaterial {
    ExtraMaterial([
        emission[0].to_bits(),
        emission[1].to_bits(),
        emission[2].to_bits(),
        strength.to_bits(),
    ])
}
fn scene(
    emission: [f32; 3],
    strength: f32,
    maps: Option<(u8, [u8; 3], bool)>,
) -> Scene<ExtraMaterial> {
    let mut s = Scene::default();
    s.radius = 1.5;
    s.surfaces.push(Surface {
        positions: vec![
            [-1., -1., 0.],
            [1., -1., 0.],
            [1., 1., 0.],
            [-1., -1., 0.],
            [1., 1., 0.],
            [-1., 1., 0.],
        ],
        normals: vec![[0., 0., 1.]; 6],
        uvs: vec![[0., 0.], [1., 0.], [1., 1.], [0., 0.], [1., 1.], [0., 1.]],
        colour: [0.4, 0.5, 0.6],
        material: material(emission, strength),
        ..Default::default()
    });
    if let Some((ao, emit, explicit)) = maps {
        s.surfaces[0].occlusion_map = Some(0);
        s.surfaces[0].emission_map = Some(1);
        for (slot, rgba) in [[ao, 17, 231, 0], [emit[0], emit[1], emit[2], 0]]
            .into_iter()
            .enumerate()
        {
            let image = Image {
                width: 1,
                height: 1,
                rgba: rgba.to_vec(),
            };
            s.images.push(image.clone());
            if explicit {
                s.explicit_images.insert(
                    slot,
                    ExplicitImage::new(
                        vec![image],
                        if slot == 0 {
                            ImageEncoding::Linear
                        } else {
                            ImageEncoding::Srgb
                        },
                    )
                    .unwrap(),
                );
            }
        }
    }
    s
}
fn viewer(s: Scene<ExtraMaterial>, ambient: f32) -> Viewer<ExtraMaterial> {
    let mut style = Style::default();
    style.lighting.ambient *= ambient;
    Viewer::new(
        Arc::new(s),
        Camera {
            yaw: 0.,
            pitch: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        },
    )
    .style(style)
    .edges(false)
}

#[test]
fn gcad_extra_maps_contract_and_refusal() {
    for strength in [f32::NAN, -0.1, 1.1] {
        assert!(
            scene([0.; 3], strength, None)
                .validate_explicit_images()
                .is_err()
        );
    }
    let mut s = scene([0.2; 3], 1., Some((128, [128; 3], true)));
    assert!(s.validate_explicit_images().is_ok());
    s.explicit_images.remove(&1);
    assert!(s.validate_explicit_images().is_err());
    s.explicit_images.clear();
    s.surfaces[0].emission_map = Some(2);
    assert!(s.validate_explicit_images().is_err());
    let image = || Image {
        width: 1,
        height: 1,
        rgba: vec![128; 4],
    };
    let linear = ExplicitImage::new(vec![image()], ImageEncoding::Linear).unwrap();
    let srgb = ExplicitImage::new(vec![image()], ImageEncoding::Srgb).unwrap();
    assert!(
        explicit_image::explicit_material_lod_with_extras([None, None, None, Some(&srgb), None])
            .is_err()
    );
    assert!(
        explicit_image::explicit_material_lod_with_extras([None, None, None, None, Some(&linear)])
            .is_err()
    );
    let long = ExplicitImage::new(
        vec![
            Image {
                width: 2,
                height: 2,
                rgba: vec![255; 16],
            },
            image(),
        ],
        ImageEncoding::Srgb,
    )
    .unwrap();
    assert_eq!(
        explicit_image::explicit_material_lod_with_extras([
            Some(&long),
            None,
            None,
            Some(&linear),
            None
        ]),
        Ok(0)
    );
    for explicit in [false, true] {
        let v = viewer(scene([0.2; 3], 1., Some((128, [128; 3], explicit))), 1.)
            .mode(RenderMode::Traversal);
        assert!(
            v.explicit_image_error()
                .unwrap()
                .contains("occlusion/emission")
        );
        assert!(v.render_gpu(16, 16).is_none());
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| v.render_soft(16, 16)))
                .is_err()
        );
    }
}

#[test]
fn gcad_extra_maps_gpu_matches_uniform_references() {
    let (backends, expected) = match std::env::var("GCAD_RENDER_BACKEND").as_deref() {
        Ok("dx12") => (wgpu::Backends::DX12, wgpu::Backend::Dx12),
        Ok("vulkan") => (wgpu::Backends::VULKAN, wgpu::Backend::Vulkan),
        other => panic!("explicit backend required: {other:?}"),
    };
    let egui_wgpu::WgpuSetup::CreateNew(mut setup) = egui_kittest::wgpu::default_wgpu_setup()
    else {
        panic!("new setup required")
    };
    setup.instance_descriptor.backends = backends;
    let state = egui_kittest::wgpu::create_render_state(
        egui_wgpu::WgpuSetup::CreateNew(setup),
        egui_wgpu::RendererOptions::PREDICTABLE,
    );
    assert_eq!(state.adapter.get_info().backend, expected);
    install(&state);
    let factor = [0.3, 0.15, 0.1];
    let texel = [128, 64, 192];
    // Independent scalar sRGB transfer and ambient scale, no map in reference.
    let decode = |v: u8| {
        let c = v as f32 / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let emit = std::array::from_fn(|i| factor[i] * decode(texel[i]));
    for explicit in [false, true] {
        let mut compared = 0;
        let mut maximum = 0;
        let mut controls = [0; 2];
        for ao in [0, 128, 255] {
            for strength in [0., 0.5, 1.] {
                let actual = viewer(scene(factor, strength, Some((ao, texel, explicit))), 1.)
                    .render_gpu(64, 64)
                    .unwrap();
                let reference = viewer(
                    scene(emit, 1., None),
                    1. - strength + strength * (ao as f32 / 255.),
                )
                .render_gpu(64, 64)
                .unwrap();
                for (a, b) in actual.pixels.iter().zip(&reference.pixels) {
                    for (a, b) in a.to_array().into_iter().zip(b.to_array()) {
                        maximum = maximum.max(a.abs_diff(b));
                    }
                }
                compared += actual.pixels.len();
                for (slot, map) in [(255, texel), (ao, [255; 3])].into_iter().enumerate() {
                    let control =
                        viewer(scene(factor, strength, Some((map.0, map.1, explicit))), 1.)
                            .render_gpu(64, 64)
                            .unwrap();
                    controls[slot] += actual
                        .pixels
                        .iter()
                        .zip(&control.pixels)
                        .filter(|(a, b)| a != b)
                        .count();
                }
            }
        }
        assert!(maximum <= 2, "extra maps reference error {maximum}");
        assert!(controls.iter().all(|&n| n > 100));
        println!(
            "Extra maps {expected:?}, explicit {explicit}: {compared} pixels, maximum error {maximum}, controls {controls:?}"
        );
    }
    // A short occlusion chain must bound the emission sampler as well. Poisoned
    // emission lower levels make failure observable, not just a metadata check.
    let render_chain = |short: bool| {
        let mut s = scene(factor, 1., Some((128, texel, true)));
        for slot in 0..2 {
            let rgba = if slot == 0 {
                [128, 17, 231, 0]
            } else {
                [128, 64, 192, 0]
            };
            let base = Image {
                width: 128,
                height: 128,
                rgba: rgba.repeat(128 * 128),
            };
            let mut levels = vec![base.clone()];
            if !(slot == 0 && short) {
                let lower = if slot == 0 { rgba } else { [255, 0, 0, 0] };
                levels.push(Image {
                    width: 64,
                    height: 64,
                    rgba: lower.repeat(64 * 64),
                });
            }
            s.images[slot] = base;
            s.explicit_images.insert(
                slot,
                ExplicitImage::new(
                    levels,
                    if slot == 0 {
                        ImageEncoding::Linear
                    } else {
                        ImageEncoding::Srgb
                    },
                )
                .unwrap(),
            );
        }
        viewer(s, 1.).render_gpu(16, 16).unwrap()
    };
    let bounded = render_chain(true);
    let unbounded = render_chain(false);
    let reference = viewer(scene(emit, 1., None), 128. / 255.)
        .render_gpu(16, 16)
        .unwrap();
    let error = bounded
        .pixels
        .iter()
        .zip(&reference.pixels)
        .flat_map(|(a, b)| {
            a.to_array()
                .into_iter()
                .zip(b.to_array())
                .map(|(a, b)| a.abs_diff(b))
        })
        .max()
        .unwrap();
    let changed = bounded
        .pixels
        .iter()
        .zip(&unbounded.pixels)
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        error <= 2 && changed > 10,
        "extra-map LOD bound ineffective: error {error}, changed {changed}"
    );
    println!(
        "Extra maps {expected:?} shortest occlusion LOD: error {error}, lower emission control {changed}"
    );
}
