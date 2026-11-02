//! Proposed egui_bench viewer3d API. This file is compiled by the gcad GPU probe
//! before any renderer dependency patch is adopted.
use super::Image;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageEncoding {
    Srgb,
    Linear,
}

/// Validated contiguous levels with a fixed clamp/linear/non-anisotropic policy.
/// Construction does not generate or repair mip levels. Colour RGB must be sRGB;
/// numeric maps and alpha are linear. Sampling a Srgb image already decodes RGB.
pub struct ExplicitImage {
    levels: Vec<Image>,
    encoding: ImageEncoding,
}

pub struct UploadedImage {
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

/// All present maps in an explicit material must use this path. The existing
/// renderer shares one sampler, so use the shortest chain's safe LOD bound.
/// Slots are base colour, tangent normal and packed metallic/roughness.
pub fn explicit_material_lod(maps: [Option<&ExplicitImage>; 3]) -> Result<u32, &'static str> {
    explicit_material_lod_with_extras([maps[0], maps[1], maps[2], None, None])
}

/// Colour, normal, metallic/roughness, occlusion, emission. All present chains
/// share the shortest safe LOD; colour and emission are sRGB, others linear.
pub fn explicit_material_lod_with_extras(maps: [Option<&ExplicitImage>; 5]) -> Result<u32, &'static str> {
    let mut max_lod = None;
    for (slot, image) in maps.into_iter().enumerate() {
        if let Some(image) = image {
            let expected = if slot == 0 || slot == 4 {
                ImageEncoding::Srgb
            } else {
                ImageEncoding::Linear
            };
            if image.encoding() != expected {
                return Err("explicit material colour/numeric encoding mismatch");
            }
            max_lod = Some(max_lod.map_or(image.max_lod(), |old: u32| old.min(image.max_lod())));
        }
    }
    max_lod.ok_or("explicit material needs at least one map")
}

pub fn explicit_sampler(device: &wgpu::Device, max_lod: u32) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("explicit image bounded sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        lod_min_clamp: 0.,
        lod_max_clamp: max_lod as f32,
        anisotropy_clamp: 1,
        ..Default::default()
    })
}

impl ExplicitImage {
    pub fn new(levels: Vec<Image>, encoding: ImageEncoding) -> Result<Self, &'static str> {
        let base = levels.first().ok_or("explicit image needs a base level")?;
        if base.width == 0 || base.height == 0 || base.width > 8192 || base.height > 8192 {
            return Err("explicit image dimensions must be 1..8192");
        }
        if levels.len() > base.width.max(base.height).ilog2() as usize + 1 {
            return Err("explicit image has too many mip levels");
        }
        let mut bytes = 0usize;
        for (level, image) in levels.iter().enumerate() {
            if image.width != (base.width >> level).max(1)
                || image.height != (base.height >> level).max(1)
                || image.rgba.len() != image.width * image.height * 4
            {
                return Err("explicit image mip dimensions or byte count mismatch");
            }
            bytes += image.rgba.len();
            if bytes > 256 * 1024 * 1024 {
                return Err("explicit image exceeds 256 MiB");
            }
        }
        Ok(Self { levels, encoding })
    }

    pub fn levels(&self) -> &[Image] {
        &self.levels
    }
    pub fn encoding(&self) -> ImageEncoding {
        self.encoding
    }
    pub fn max_lod(&self) -> u32 {
        self.levels.len() as u32 - 1
    }

    pub fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<UploadedImage, &'static str> {
        let base = &self.levels[0];
        if base.width.max(base.height) > device.limits().max_texture_dimension_2d as usize {
            return Err("explicit image exceeds GPU texture dimension limit");
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("explicit image levels"),
            size: wgpu::Extent3d {
                width: base.width as u32,
                height: base.height as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: self.levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: match self.encoding {
                ImageEncoding::Srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
                ImageEncoding::Linear => wgpu::TextureFormat::Rgba8Unorm,
            },
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, image) in self.levels.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &image.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(image.width as u32 * 4),
                    rows_per_image: Some(image.height as u32),
                },
                wgpu::Extent3d {
                    width: image.width as u32,
                    height: image.height as u32,
                    depth_or_array_layers: 1,
                },
            );
        }
        let sampler = explicit_sampler(device, self.max_lod());
        Ok(UploadedImage {
            view: texture.create_view(&Default::default()),
            sampler,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image(w: usize, h: usize) -> Image {
        Image {
            width: w,
            height: h,
            rgba: vec![0; w * h * 4],
        }
    }
    #[test]
    fn accepts_partial_npot_chain_and_exposes_exact_bound() {
        let data = ExplicitImage::new(
            vec![image(31, 17), image(15, 8), image(7, 4)],
            ImageEncoding::Srgb,
        )
        .unwrap();
        assert_eq!(data.max_lod(), 2);
        assert_eq!(data.levels()[2].width, 7);
        assert_eq!(data.encoding(), ImageEncoding::Srgb);
        assert_eq!(
            ExplicitImage::new(vec![image(1, 1)], ImageEncoding::Linear)
                .unwrap()
                .max_lod(),
            0
        );
    }
    #[test]
    fn rejects_malformed_and_noncontiguous_levels() {
        for levels in [
            vec![],
            vec![image(0, 1)],
            vec![image(8193, 1)],
            vec![image(4, 4), image(1, 1)],
            vec![image(1, 1), image(1, 1)],
            vec![Image {
                width: 2,
                height: 2,
                rgba: vec![0; 15],
            }],
        ] {
            assert!(ExplicitImage::new(levels, ImageEncoding::Linear).is_err());
        }
    }
    #[test]
    fn shared_material_sampler_uses_shortest_chain_and_correct_encodings() {
        let colour = ExplicitImage::new(
            vec![image(8, 8), image(4, 4), image(2, 2)],
            ImageEncoding::Srgb,
        )
        .unwrap();
        let normal = ExplicitImage::new(vec![image(8, 8)], ImageEncoding::Linear).unwrap();
        assert_eq!(explicit_material_lod([Some(&colour), None, None]), Ok(2));
        assert_eq!(
            explicit_material_lod([Some(&colour), Some(&normal), None]),
            Ok(0)
        );
        assert!(explicit_material_lod([Some(&normal), None, None]).is_err());
        assert!(explicit_material_lod([Some(&colour), Some(&colour), None]).is_err());
        assert!(explicit_material_lod([None, None, None]).is_err());
    }
}
