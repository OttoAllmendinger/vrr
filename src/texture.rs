use crate::image_loader::ImageRef;
use crate::image_loader::ImageRequest;
use anyhow::*;
use image::{DynamicImage, ImageBuffer};
use log::{debug, error};
use logging_timer::{executing, time, timer};
use number_prefix::NumberPrefix;
use rexiv2::{Metadata, Orientation};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum ImageResolution {
    THUMBNAIL,
    FULLHD,
    NATIVE,
}

pub struct Texture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
    Rgb,
    Rgba,
    Yuv,
    Raw,
}

pub struct DecodeStats {
    bytes: usize,
    elapsed: std::time::Duration,
}

impl DecodeStats {
    pub fn new(bytes: usize, elapsed: std::time::Duration) -> Self {
        Self { bytes, elapsed }
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.elapsed.as_secs_f64() * 1000.0
    }

    pub fn bytes_si(&self) -> String {
        match NumberPrefix::decimal(self.bytes as f64) {
            NumberPrefix::Standalone(bytes) => format!("{:.0} B", bytes),
            NumberPrefix::Prefixed(prefix, n) => format!("{:.1} {}B", n, prefix),
        }
    }

    pub fn bytes_per_sec_si(&self) -> String {
        let bytes_per_sec = self.bytes as f64 / self.elapsed.as_secs_f64();
        match NumberPrefix::decimal(bytes_per_sec) {
            NumberPrefix::Standalone(bytes) => format!("{:.0} B/s", bytes),
            NumberPrefix::Prefixed(prefix, n) => format!("{:.1} {}B/s", n, prefix),
        }
    }
}

impl Texture {
    #[time]
    pub fn from_rgba(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: DynamicImage,
        label: Option<&str>,
    ) -> Result<Self> {
        let tmr = timer!("creating texture");
        let size = wgpu::Extent3d {
            width: image.width(),
            height: image.height(),
            depth_or_array_layers: 1,
        };
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        executing!(tmr, "texture created");

        queue.write_texture(
            wgpu::ImageCopyTexture {
                aspect: wgpu::TextureAspect::All,
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
            },
            image.as_bytes(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * image.width()),
                rows_per_image: Some(image.height()),
            },
            size,
        );
        executing!(tmr, "texture written");

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        executing!(tmr, "texture view created");

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        executing!(tmr, "sampler created");

        Ok(Self {
            texture,
            view,
            sampler,
        })
    }

    #[time]
    pub fn from_image(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        img: DynamicImage,
        label: Option<&str>,
    ) -> Result<Self> {
        Self::from_rgba(device, queue, img, label)
    }

    pub fn decode_turbojpeg(
        bytes: &[u8],
        format: turbojpeg::PixelFormat,
    ) -> Result<(u32, u32, Vec<u8>)> {
        let img = turbojpeg::decompress(bytes, format)?;
        Ok((img.width as u32, img.height as u32, img.pixels))
    }
}

pub fn decode_turbojpeg(
    bytes: &[u8],
    scale: u8,
    color_space: ColorSpace,
) -> Result<(u32, u32, Vec<u8>)> {
    if scale != 8 {
        return Err(anyhow!("Unsupported scale"));
    }
    let format = match color_space {
        ColorSpace::Rgb => Ok(turbojpeg::PixelFormat::RGB),
        ColorSpace::Rgba => Ok(turbojpeg::PixelFormat::RGBA),
        _ => Err(anyhow!("Unsupported color space")),
    }?;
    let result = std::panic::catch_unwind(|| {
        let tmr = timer!("Decompress JPEG");
        let img = turbojpeg::decompress(bytes, format)?;
        executing!(tmr, "decompress init complete");
        let (w, h) = (img.width as u32, img.height as u32);
        executing!(tmr, "read scanlines complete {:?}", format);
        Ok((w, h, img.pixels))
    })
    .map_err(|err| anyhow!("Failed to decompress JPEG: {:?}", err))?;

    result
}

pub fn load_image_thumbnail_bytes(metadata: &Metadata) -> Option<Vec<u8>> {
    metadata.get_thumbnail().map(|t| t.to_vec())
}

#[time]
pub fn load_image_bytes(path: PathBuf) -> Vec<u8> {
    use std::fs::File;
    use std::io::Read;
    let mut file = File::open(&path).unwrap();
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).unwrap();
    buffer
}

fn check_color_space(path: &PathBuf, metadata: &Metadata) {
    match metadata.get_tag_string("Exif.Photo.ColorSpace") {
        Result::Ok(s) if s.eq("1") => {
            debug!("{}: Color space: sRGB", path.display());
        }
        Result::Ok(s) => {
            error!("{}: Unknown color space: {}", path.display(), s)
        }
        Err(e) => {
            error!("{}: Failed to get color space: {}", path.display(), e)
        }
    }
}

pub fn get_rgba_for_path(
    path: PathBuf,
    resolution: &ImageResolution,
) -> Result<(DynamicImage, Orientation)> {
    let metadata = Metadata::new_from_path(&path)?;
    check_color_space(&path, &metadata);
    let orientation = metadata.get_orientation();
    let img_bytes = match resolution {
        ImageResolution::THUMBNAIL => {
            load_image_thumbnail_bytes(&metadata).unwrap_or_else(|| load_image_bytes(path.clone()))
        }
        ImageResolution::NATIVE => load_image_bytes(path.clone()),
        ImageResolution::FULLHD => todo!(),
    };
    let start_time = std::time::Instant::now();
    let (w, h, bytes) = decode_turbojpeg(&img_bytes, 8, ColorSpace::Rgba)?;
    let elapsed = start_time.elapsed();
    let decode_stats = DecodeStats::new(bytes.len(), elapsed);
    debug!(
        "Decompressed JPEG, {}x{}px, {}ms, {}, {}",
        w,
        h,
        decode_stats.elapsed_ms(),
        decode_stats.bytes_si(),
        decode_stats.bytes_per_sec_si()
    );
    Ok((
        DynamicImage::ImageRgba8(ImageBuffer::from_vec(w, h, bytes).unwrap()),
        orientation,
    ))
}

#[derive(Debug)]
pub struct SizedImage {
    pub image_ref: ImageRef,
    pub resolution: ImageResolution,
    pub orientation: Orientation,
    pub image: DynamicImage,
}

impl SizedImage {
    pub fn from_request(image_request: ImageRequest) -> Result<Self> {
        let (image, orientation) = get_rgba_for_path(
            image_request.reference.path.clone(),
            &image_request.resolution,
        )?;
        Ok(Self {
            image_ref: image_request.reference,
            resolution: image_request.resolution,
            orientation,
            image,
        })
    }
}
