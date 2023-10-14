use glyphon::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer,
};
use glyphon::cosmic_text::Align;
use log::debug;
use wgpu::{
    CommandEncoderDescriptor, LoadOp, Operations, RenderPassColorAttachment, RenderPassDescriptor,
    TextureViewDescriptor,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HorizontalPosition {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerticalPosition {
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position(HorizontalPosition, VerticalPosition);

impl Position {
    pub fn new(h: HorizontalPosition, v: VerticalPosition) -> Self {
        Self(h, v)
    }
}

pub struct OverlayElement {
    pub position: Position,
    pub text: String,
    pub size: winit::dpi::PhysicalSize<u32>,
    pub buffer: Buffer,
    pub text_renderer: TextRenderer,
    pub offset: (f32, f32),
}

impl OverlayElement {
    fn offset_for_position(p: Position) -> (f32, f32) {
        let offset = 10.0;
        let offset_x = match p.0 {
            HorizontalPosition::Left => offset,
            HorizontalPosition::Center => 0.0,
            HorizontalPosition::Right => -offset,
        };
        let offset_y = match p.1 {
            VerticalPosition::Top => offset,
            VerticalPosition::Center => 0.0,
            VerticalPosition::Bottom => -offset,
        };
        (offset_x, offset_y)
    }
    pub fn new(
        text_renderer: TextRenderer,
        font_system: &mut FontSystem,
        position: Position,
        metrics: Metrics) -> Self {
        Self {
            text_renderer,
            position,
            text: String::new(),
            size: winit::dpi::PhysicalSize::new(0, 0),
            buffer: Buffer::new(font_system, metrics),
            offset: Self::offset_for_position(position),
        }
    }

    pub fn update(
        &mut self,
        font_system: &mut FontSystem,
        text: String,
        size: winit::dpi::PhysicalSize<u32>,
        align: Option<Align>,
    ) -> bool {
        if self.text == text && self.size == size {
            return false;
        }
        self.buffer.set_size(font_system, size.width as f32, size.height as f32);
        self.buffer.set_text(
            font_system,
            &text,
            Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );
        for l in &mut self.buffer.lines {
            l.set_align(align);
        }
        self.buffer.shape_until_scroll(font_system);
        if self.position.1 == VerticalPosition::Bottom {
            let total_height = self.buffer.lines.len() as f32 * self.buffer.metrics().line_height;
            self.offset.1 = self.size.height as f32 - total_height - 10.0;
        }

        self.text = text;
        self.size = size;
        true
    }
}

pub struct Overlay {
    font_system: FontSystem,
    atlas: TextAtlas,
    cache: SwashCache,
    elements: Vec<OverlayElement>,
}

impl Overlay {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        swapchain_format: wgpu::TextureFormat,
    ) -> Self {
        let font_system = FontSystem::new();
        let cache = SwashCache::new();
        let atlas = TextAtlas::new(device, queue, swapchain_format);
        Self {
            font_system,
            atlas,
            cache,
            elements: Vec::new(),
        }
    }

    fn textarea_at_offset(
        buffer: &Buffer,
        offset: (f32, f32),
        scale: f32,
        color: Color,
        bounds: TextBounds,
    ) -> TextArea {
        TextArea {
            buffer,
            left: offset.0,
            top: offset.1,
            scale,
            bounds,
            default_color: color,
        }
    }

    fn textareas_outline(
        buffer: &Buffer,
        scale: f32,
        offset: (f32, f32),
        radius: i32,
        color_outline: Color,
        color_center: Color,
        bounds: TextBounds,
    ) -> Vec<TextArea> {
        let mut textareas = Vec::new();
        for x in -radius..=radius {
            for y in -radius..=radius {
                if x != 0 && y != 0 {
                    textareas.push(Self::textarea_at_offset(
                        buffer,
                        (offset.0 + x as f32, offset.1 + y as f32),
                        scale,
                        color_outline,
                        bounds,
                    ));
                }
            }
        }
        textareas.push(Self::textarea_at_offset(
            buffer,
            offset,
            scale,
            color_center,
            bounds,
        ));
        textareas
    }

    fn get_align(pos: Position) -> Align {
        match pos.0 {
            HorizontalPosition::Left => Align::Left,
            HorizontalPosition::Center => Align::Center,
            HorizontalPosition::Right => Align::Right,
        }
    }

    pub fn default_metrics() -> Metrics {
        Metrics::new(30.0, 42.0)
    }

    pub fn element_at_position<'a>(
        elements: &'a mut Vec<OverlayElement>,
        p: Position,
        font_system: &mut FontSystem,
        atlas: &mut TextAtlas,
        device: &wgpu::Device,
    ) -> &'a mut OverlayElement {
        let index = elements.iter().position(|e| e.position == p);
        match index {
            Some(i) => &mut elements[i],
            None => {
                let text_renderer = TextRenderer::new(
                    atlas,
                    device,
                    wgpu::MultisampleState::default(),
                    None);

                let metrics = Self::default_metrics();
                let element = OverlayElement::new(text_renderer, font_system, p, metrics);
                elements.push(element);
                elements.last_mut().unwrap()
            }
        }
    }

    pub fn update(
        &mut self,
        position: Position,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: &winit::dpi::PhysicalSize<u32>,
        text: String,
    ) {
        let element = Self::element_at_position(&mut self.elements, position, &mut self.font_system, &mut self.atlas, device);
        if !element.update(&mut self.font_system, text.clone(), *size, Some(Self::get_align(position))) {
            return;
        }
        debug!("Updating overlay text={} size={:?} prev={}/{:?}", text, size, element.text, element.size);
        let textareas = Self::textareas_outline(
            &element.buffer,
            1.0,
            element.offset,
            2,
            Color::rgb(0, 0, 0),
            Color::rgb(255, 255, 255),
            TextBounds::default(),
        );
        element.text_renderer
            .prepare(
                &device,
                &queue,
                &mut self.font_system,
                &mut self.atlas,
                Resolution {
                    width: size.width,
                    height: size.height,
                },
                textareas,
                &mut self.cache,
            )
            .unwrap();
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_texture: &wgpu::SurfaceTexture,
    ) {
        let view = surface_texture
            .texture
            .create_view(&TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
            });

            for element in &mut self.elements {
                element.text_renderer.render(&self.atlas, &mut pass).unwrap();
            }
        }

        queue.submit(Some(encoder.finish()));

        self.atlas.trim();
    }
}
