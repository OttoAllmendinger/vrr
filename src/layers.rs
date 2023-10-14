use crate::image_loader::{ImageRef, ImageRequest};
use crate::texture;
use crate::texture::{ImageResolution, SizedImage};
use crate::viewport::Uniforms;
use anyhow::*;
use bytemuck::Zeroable;
use log::debug;
use logging_timer::time;
use number_prefix::NumberPrefix;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

pub type Orientation = rexiv2::Orientation;

pub struct Layer {
    pub image_ref: ImageRef,
    pub resolution: ImageResolution,
    pub orientation: Orientation,
    pub texture_bind_group: wgpu::BindGroup,
    pub texture: wgpu::Texture,
    pub uniform_bind_group: wgpu::BindGroup,
    pub uniform_buffer: wgpu::Buffer,
}

impl Layer {
    fn texture_byte_size(&self) -> usize {
        self.texture.width() as usize * self.texture.height() as usize * 4
    }
}

pub struct Layers {
    pub texture_bind_group_layout: wgpu::BindGroupLayout,
    pub uniform_bind_group_layout: wgpu::BindGroupLayout,
    pub layers: HashMap<ImageRef, Vec<Layer>>,
}

impl Layers {
    pub fn new(
        texture_bind_group_layout: wgpu::BindGroupLayout,
        uniform_bind_group_layout: wgpu::BindGroupLayout,
    ) -> Self {
        Self {
            texture_bind_group_layout,
            uniform_bind_group_layout,
            layers: HashMap::new(),
        }
    }

    #[time]
    fn bind_group_for_texture(
        &self,
        device: &wgpu::Device,
        diffuse_texture: &texture::Texture,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &self.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                },
            ],
            label: Some("diffuse_bind_group"),
        })
    }

    #[time]
    fn create_layer(
        &self,
        device: &wgpu::Device,
        image_ref: ImageRef,
        resolution: ImageResolution,
        orientation: rexiv2::Orientation,
        texture: texture::Texture,
    ) -> Result<Layer> {
        let texture_bind_group = self.bind_group_for_texture(device, &texture);

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::bytes_of(&Uniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        Ok(Layer {
            image_ref,
            resolution,
            orientation,
            texture_bind_group: texture_bind_group,
            texture: texture.texture,
            uniform_bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.uniform_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                }],
                label: Some("uniform_bind_group"),
            }),
            uniform_buffer,
        })
    }

    #[time]
    pub fn create_layer_from_sized_image(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sized_image: SizedImage,
    ) -> Result<Layer> {
        let texture = texture::Texture::from_rgba(&device, &queue, sized_image.image, None)?;
        self.create_layer(
            device,
            sized_image.image_ref,
            sized_image.resolution,
            sized_image.orientation,
            texture,
        )
    }

    fn get_best_layer<'a, I>(layers: I) -> Option<&'a Layer>
    where
        I: Iterator<Item = &'a Layer>,
    {
        layers.max_by_key(|l| l.resolution)
    }

    pub fn get_layer(&self, iref: &ImageRef) -> Option<&Layer> {
        Self::get_best_layer(self.layers.get(iref)?.iter())
    }

    pub fn retain(&mut self, reqs: &[ImageRequest]) {
        self.layers.retain(|iref, layers| {
            layers.retain(|l| {
                reqs.iter()
                    .any(|req| req.reference == *iref && req.resolution == l.resolution)
            });
            !layers.is_empty()
        });
    }

    fn dump_layer_info(&self) {
        let total_texture_bytes = self
            .layers
            .values()
            .flatten()
            .map(|l| l.texture_byte_size())
            .reduce(|a, b| a + b);

        debug!("Layers: {}", self.layers.len());
        debug!(
            "Texture Bytes: {}",
            total_texture_bytes
                .map(|b| match NumberPrefix::decimal(b as f64) {
                    NumberPrefix::Standalone(bytes) => format!("{:.0} B", bytes),
                    NumberPrefix::Prefixed(prefix, n) => format!("{:.1} {}B", n, prefix),
                })
                .unwrap_or_else(|| "???".to_string())
        );
    }

    pub fn add_layer(&mut self, layer: Layer) {
        debug!("Adding layer: {:?}", layer.image_ref);
        match self.layers.get_mut(&layer.image_ref) {
            Some(layers) => {
                layers.retain(|l| l.resolution != layer.resolution);
                layers.push(layer);
            }
            None => {
                self.layers.insert(layer.image_ref.clone(), vec![layer]);
            }
        }
        // self.prune_layers(4);
        self.dump_layer_info();
    }

    pub fn add_layer_from_sized_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sized_image: SizedImage,
    ) -> Result<()> {
        let layer = self.create_layer_from_sized_image(device, queue, sized_image)?;
        self.add_layer(layer);
        Ok(())
    }
}
