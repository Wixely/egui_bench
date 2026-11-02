use super::*;

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
struct AlphaMaterial {
    mode: u8,
    factor: u32,
    cutoff: u32,
}
impl Material for AlphaMaterial {
    fn pbr(&self) -> Pbr {
        Pbr {
            alpha: match self.mode {
                1 => AlphaMode::Blend,
                2 => AlphaMode::Mask {
                    cutoff: f32::from_bits(self.cutoff),
                },
                _ => AlphaMode::Opaque,
            },
            base_alpha: f32::from_bits(self.factor),
            ..Default::default()
        }
    }
}
fn material(mode: u8, factor: f32, cutoff: f32) -> AlphaMaterial {
    AlphaMaterial {
        mode,
        factor: factor.to_bits(),
        cutoff: cutoff.to_bits(),
    }
}
fn quad(z: f32, colour: [f32; 3], material: AlphaMaterial) -> Surface<AlphaMaterial> {
    Surface {
        positions: vec![
            [-1., -1., z],
            [1., -1., z],
            [1., 1., z],
            [-1., -1., z],
            [1., 1., z],
            [-1., 1., z],
        ],
        normals: vec![[0., 0., 1.]; 6],
        uvs: vec![[0., 0.], [1., 0.], [1., 1.], [0., 0.], [1., 1.], [0., 1.]],
        colour,
        material,
        ..Default::default()
    }
}
fn scene(
    alpha: AlphaMaterial,
    pixels: [u8; 2],
    explicit: bool,
    front: bool,
) -> Scene<AlphaMaterial> {
    let mut scene = Scene::default();
    scene.radius = 1.5;
    scene
        .surfaces
        .push(quad(0., [0.1, 0.2, 0.8], material(0, 1., 0.)));
    if front {
        let mut surface = quad(0.1, [1.; 3], alpha);
        surface.texture = Some(0);
        surface.group = 1;
        scene.surfaces.push(surface);
        let base = Image {
            width: 2,
            height: 1,
            rgba: pixels.into_iter().flat_map(|a| [220, 60, 30, a]).collect(),
        };
        scene.images.push(base.clone());
        if explicit {
            scene.explicit_images.insert(
                0,
                ExplicitImage::new(vec![base], ImageEncoding::Srgb).unwrap(),
            );
        }
    }
    scene
}
fn viewer(scene: Scene<AlphaMaterial>) -> Viewer<AlphaMaterial> {
    Viewer::new(
        Arc::new(scene),
        Camera {
            yaw: 0.,
            pitch: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        },
    )
    .edges(false)
}
fn maximum(a: &egui::ColorImage, b: &egui::ColorImage) -> u8 {
    a.pixels
        .iter()
        .zip(&b.pixels)
        .flat_map(|(a, b)| {
            a.to_array()
                .into_iter()
                .zip(b.to_array())
                .map(|(a, b)| a.abs_diff(b))
        })
        .max()
        .unwrap()
}

#[test]
fn gcad_alpha_validates_and_refuses_unsupported_modes() {
    for bad in [
        material(1, f32::NAN, 0.),
        material(1, 1.1, 0.),
        material(2, 1., -0.1),
        material(2, 1., f32::NAN),
    ] {
        assert!(
            scene(bad, [255; 2], false, true)
                .validate_explicit_images()
                .is_err()
        );
    }
    let value =
        viewer(scene(material(2, 1., 0.5), [255; 2], false, true)).mode(RenderMode::Traversal);
    assert!(value.explicit_image_error().unwrap().contains("alpha"));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| value.render_soft(8, 8))).is_err()
    );
}

#[test]
fn gcad_alpha_masks_depth_and_matches_uniform_blend_reference() {
    let (backends, expected) = match std::env::var("GCAD_RENDER_BACKEND").as_deref() {
        Ok("vulkan") => (wgpu::Backends::VULKAN, wgpu::Backend::Vulkan),
        Ok("dx12") | Err(std::env::VarError::NotPresent) => {
            (wgpu::Backends::DX12, wgpu::Backend::Dx12)
        }
        other => panic!("unsupported backend {other:?}"),
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
    gpu::install(&state);
    let render = |v: Viewer<AlphaMaterial>| v.render_gpu(96, 96).expect("alpha test requires GPU");
    let background = render(viewer(scene(material(0, 1., 0.), [255; 2], false, false)));
    let opaque = render(viewer(scene(material(0, 1., 0.), [255; 2], false, true)));
    for explicit in [false, true] {
        for (a, factor, cutoff, visible) in [
            (0, 1., 0.5, false),
            (127, 1., 0.5, false),
            (128, 1., 0.5, true),
            (255, 0.5, 0.5, true),
            (255, 0.49, 0.5, false),
            (0, 0., 0., true),
        ] {
            let image = render(viewer(scene(
                material(2, factor, cutoff),
                [a; 2],
                explicit,
                true,
            )));
            assert!(
                maximum(&image, if visible { &opaque } else { &background }) <= 2,
                "mask threshold/depth mismatch"
            );
        }
        let ignored = render(viewer(scene(material(0, 0., 0.), [0; 2], explicit, true)));
        assert!(
            maximum(&ignored, &opaque) <= 2,
            "opaque must ignore texture and factor alpha"
        );
        for (a, factor, look) in [(0, 1., 1.), (64, 1., 1.), (128, 0.5, 0.7), (255, 1., 1.)] {
            let image = render(
                viewer(scene(material(1, factor, 0.), [a; 2], explicit, true)).looks(vec![
                    Look::default(),
                    Look {
                        opacity: look,
                        ..Default::default()
                    },
                ]),
            );
            let expected = render(
                viewer(scene(material(0, 1., 0.), [255; 2], false, true)).looks(vec![
                    Look::default(),
                    Look {
                        opacity: a as f32 / 255. * factor * look,
                        ..Default::default()
                    },
                ]),
            );
            assert!(
                maximum(&image, &expected) <= 2,
                "blend texture/factor/look product mismatch"
            );
        }
        let mixed = render(viewer(scene(
            material(2, 1., 0.5),
            [0, 255],
            explicit,
            true,
        )));
        let front = mixed
            .pixels
            .iter()
            .zip(&opaque.pixels)
            .zip(&background.pixels)
            .filter(|((m, o), b)| m == o && o != b)
            .count();
        let back = mixed
            .pixels
            .iter()
            .zip(&opaque.pixels)
            .zip(&background.pixels)
            .filter(|((m, o), b)| m == b && o != b)
            .count();
        assert!(
            front > 100 && back > 100,
            "mask did not expose both foreground and background"
        );
        println!(
            "Alpha raster {expected:?}, explicit {explicit}: mask foreground {front}, background {back}; threshold and blend controls passed"
        );
    }
}
