use crate::image_loader::ImageLoader;
use crate::input_events::{on_event, Inputs};
use crate::layers::{Layer, Layers};
use crate::texture::SizedImage;
use crate::viewport::{Uniforms, Viewport};
use anyhow::anyhow;
use anyhow::*;

use crate::config::Config;

use crate::overlay::{HorizontalPosition, Overlay, Position, VerticalPosition};
use crate::storage::{Storage, TAG_STARRED};
use log::debug;
use logging_timer::{executing, timer};
use std::iter;
use std::num::NonZeroU64;
use wgpu::util::DeviceExt;
use winit::event_loop::EventLoop;
use winit::window::{Window, WindowBuilder};
use crate::fps_meter::FpsMeter;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
}

impl Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

// two triangles that result in a square
const VERTICES: &[Vertex] = &[
    Vertex {
        position: [-1., 1., 0.0],
        tex_coords: [0.0, 0.0],
    }, // A
    Vertex {
        position: [-1., -1., 0.0],
        tex_coords: [0.0, 1.0],
    }, // B
    Vertex {
        position: [1., -1., 0.0],
        tex_coords: [1.0, 1.0],
    }, // C
    Vertex {
        position: [1., 1., 0.0],
        tex_coords: [1.0, 0.0],
    },
];

// 6 indices, forming two triangles
const INDICES: &[u16] = &[0, 1, 2, 0, 2, 3];

pub struct Viewer {
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    fps_meter: FpsMeter,
    pub config: Config,
    pub size: winit::dpi::PhysicalSize<u32>,
    pub loader: ImageLoader,
    pub storage: Storage,
    pub layers: Layers,
    pub view: Viewport,
    pub inputs: Inputs,
    pub overlay: Overlay,
}

impl Viewer {
    pub async fn new(window: &Window, config: Config) -> Result<Self> {
        let tmr = timer!("Renderer::new");
        let loader = ImageLoader::from_path(config.path.clone(), config.preload)?;
        let size = window.inner_size();

        executing!(tmr, "Instance::new");
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            // FIXME: wgpu::Backends::all() is more portable but a bit slower
            backends: wgpu::Backends::GL,
            dx12_shader_compiler: Default::default(),
        });

        executing!(tmr, "create_surface");
        // # Safety
        //
        // The surface needs to live as long as the window that created it.
        // State owns the window so this should be safe.
        let surface = unsafe { instance.create_surface(&window) }.unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    features: wgpu::Features::empty(),
                    // WebGL doesn't support all of wgpu's features, so if
                    // we're building for the web we'll have to disable some.
                    limits: wgpu::Limits::default(),
                },
                None, // Trace path
            )
            .await?;

        let errors = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let errors_clone = errors.clone();
        device.on_uncaptured_error(Box::new(move |e| {
            errors_clone.lock().unwrap().push(e.to_string());
        }));

        let surface_caps = surface.get_capabilities(&adapter);
        // Shader code in this tutorial assumes an Srgb surface texture. Using a different
        // one will result all the colors coming out darker. If you want to support non
        // Srgb surfaces, you'll need to account for that when drawing to the frame.
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                        count: None,
                    },
                ],
                label: Some("texture_bind_group_layout"),
            });

        let uniform_size = NonZeroU64::new(Uniforms::min_binding_size() as u64)
            .ok_or(anyhow!("uniform size is zero"))?;

        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0, // Match the binding in the shader
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(uniform_size),
                    },
                    count: None,
                }],
                label: Some("uniform_bind_group_layout"),
            });

        // read src/shader.wsgl as string using io
        let shader_source = std::fs::read_to_string("src/shader.wgsl")?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&texture_bind_group_layout, &uniform_bind_group_layout],
                push_constant_ranges: &[],
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::Zero,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                // Setting this to anything other than Fill requires Features::POLYGON_MODE_LINE
                // or Features::POLYGON_MODE_POINT
                polygon_mode: wgpu::PolygonMode::Fill,
                // Requires Features::DEPTH_CLIP_CONTROL
                unclipped_depth: false,
                // Requires Features::CONSERVATIVE_RASTERIZATION
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            // If the pipeline will be used with a multiview render pass, this
            // indicates how many array layers the attachments will have.
            multiview: None,
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        let err_total = errors
            .lock()
            .map_err(|e| anyhow!("error getting errors: {}", e))?;

        if !err_total.is_empty() {
            for e in err_total.iter() {
                log::error!("wgpu error: {}", e);
            }
            return Err(anyhow!("wgpu error"));
        }

        let overlay = Overlay::new(&device, &queue, surface_config.format);
        let storage = Storage::new()?;

        Ok(Self {
            surface,
            device,
            queue,
            surface_config,
            size,
            render_pipeline,
            vertex_buffer,
            index_buffer,
            loader,
            fps_meter: FpsMeter::new(),
            inputs: Inputs::new(),
            layers: Layers::new(texture_bind_group_layout, uniform_bind_group_layout),
            view: Viewport::new(),
            storage,
            config,
            overlay,
        })
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.surface_config.width = new_size.width;
            self.surface_config.height = new_size.height;
            self.surface.configure(&self.device, &self.surface_config);
        }
    }

    pub fn resize_fullscreen(&mut self, window: &Window) -> anyhow::Result<()> {
        // resize fullscreen
        let fullscreen = if window.fullscreen().is_some() {
            None // If already in fullscreen, revert to windowed
        } else {
            // Change to fullscreen with the current monitor
            Some(winit::window::Fullscreen::Borderless(
                window.current_monitor(),
            ))
        };
        window.set_fullscreen(fullscreen);
        Ok(())
    }

    fn draw_layer<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        layer: &'a Layer,
        alpha: f64,
    ) {
        let image_size = (layer.texture.width() as f64, layer.texture.height() as f64);
        let screen_size = (self.size.width as f64, self.size.height as f64);
        self.queue.write_buffer(
            &layer.uniform_buffer,
            0,
            bytemuck::bytes_of(&self.view.to_uniforms(
                image_size,
                screen_size,
                layer.orientation,
                alpha,
            )),
        );
        render_pass.set_bind_group(0, &layer.texture_bind_group, &[]);
        render_pass.set_bind_group(1, &layer.uniform_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        render_pass.draw_indexed(0..INDICES.len() as u32, 0, 0..1);
    }

    pub fn update_overlay(&mut self) {
        // draw filename top-left
        let filename = format!(
            "{}",
            self.loader
                .current()
                .path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
        );
        self.overlay.update(
            Position::new(HorizontalPosition::Left, VerticalPosition::Top),
            &self.device,
            &self.queue,
            &self.size,
            filename
        );

        // draw starred marker top-right

        let starred = if self.storage.entry(&self.loader.current()).has_tag(TAG_STARRED) {
            "★"
        } else {
            ""
        };
        self.overlay.update(
            Position::new(HorizontalPosition::Right, VerticalPosition::Top),
            &self.device,
            &self.queue,
            &self.size,
            starred.to_owned()
        );

        let fps = format!("{} fps", self.fps_meter.fps());
        self.overlay.update(
            Position::new(HorizontalPosition::Right, VerticalPosition::Bottom),
            &self.device,
            &self.queue,
            &self.size,
            fps
        );
    }

    pub fn render(&mut self) -> Result<()> {
        self.fps_meter.tick();
        let output = self
            .surface
            .get_current_texture()
            .expect("error creating texture");
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });

            render_pass.set_pipeline(&self.render_pipeline);

            let iref = self.loader.current();
            if let Some(layer) = self.layers.get_layer(&iref) {
                self.draw_layer(&mut render_pass, layer, 1.0);
            }
        }

        self.queue.submit(iter::once(encoder.finish()));
        self.update_overlay();
        self.overlay.render(&self.device, &self.queue, &output);
        output.present();
        Ok(())
    }

    pub fn add_image(&mut self, si: SizedImage) -> Result<()> {
        debug!("set image: {:?} {:?}", si.image_ref.path, si.resolution);
        self.layers
            .add_layer_from_sized_image(&self.device, &self.queue, si)?;

        self.loader.preload(self.loader.preload)
            .map_err(|e| anyhow!("error preloading images: {}", e))
            .ok();
        self.loader.clear_cache();
        self.layers.retain(&self.loader.cached());

        Ok(())
    }
}

pub async fn run(config: Config) {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_transparent(true)
        .build(&event_loop)
        .unwrap();

    let mut viewer = Viewer::new(&window, config).await
        .map_err(|e| log::error!("error creating viewer: {}", e))
        .unwrap();

    event_loop.run(move |event, _, control_flow| {
        for image in viewer.loader.images() {
            viewer
                .add_image(image)
                .map_err(|e| log::error!("error adding image: {}", e))
                .ok();
        }

        // set window title to filename
        {
            let iref = viewer.loader.current();
            window.set_title(&format!(
                "{} - {}",
                iref.path.file_name().unwrap().to_str().unwrap(),
                iref.path.parent().unwrap().to_str().unwrap()
            ));
        }

        pollster::block_on(on_event(&window, event, control_flow, &mut viewer));
    });
}
