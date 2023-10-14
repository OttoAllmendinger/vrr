use crate::viewer::Viewer;
use log::trace;

use winit::event::{
    DeviceEvent, ElementState, Event, KeyboardInput, MouseButton, MouseScrollDelta, TouchPhase,
    VirtualKeyCode, WindowEvent,
};
use winit::event_loop::ControlFlow;
use winit::window::Window;
use crate::storage::TAG_STARRED;

pub struct Inputs {
    mouse_pos: Option<(f64, f64)>,
    mouse_down: bool,
}

impl Inputs {
    pub fn new() -> Self {
        Self {
            mouse_pos: None,
            mouse_down: false,
        }
    }
}

async fn on_key_press(
    window: &Window,
    viewer: &mut Viewer,
    k: &VirtualKeyCode,
) -> Option<ControlFlow> {
    trace!("Key pressed: {:?}", k);
    let result = match k {
        VirtualKeyCode::Escape | VirtualKeyCode::Q => return Some(ControlFlow::Exit),
        VirtualKeyCode::J => viewer.loader.next_image(),
        VirtualKeyCode::K => viewer.loader.prev_image(),
        VirtualKeyCode::F => viewer.resize_fullscreen(window),
        VirtualKeyCode::M => {
            viewer.storage.entry(&viewer.loader.current()).toggle_tag(TAG_STARRED.to_string());
            viewer.storage.save().map_err(|e| {
                log::error!("Error saving storage: {}", e);
            }).ok();
            Ok(())
        },
        VirtualKeyCode::Minus => Ok(()),
        VirtualKeyCode::Plus => Ok(()),
        VirtualKeyCode::Equals => Ok(()),
        VirtualKeyCode::X => {
            viewer.view.zoom = 1.0;
            viewer.view.pan = (0.0, 0.0);
            Ok(())
        }
        VirtualKeyCode::R => match Viewer::new(window, viewer.config.clone()).await {
            Ok(v) => {
                *viewer = v;
                Ok(())
            }
            Err(e) => Err(e),
        },
        _ => Ok(()),
    };

    if let Err(e) = result {
        log::error!("Error: {}", e);
    }

    None
}

async fn on_mouse_button(
    _window: &Window,
    viewer: &mut Viewer,
    element_state: &ElementState,
    button: &MouseButton,
) -> Option<ControlFlow> {
    if let ElementState::Pressed = element_state {
        trace!("Mouse input: {:?} {:?}", element_state, button);
        match button {
            MouseButton::Left => {
                viewer.inputs.mouse_down = true;
            }
            MouseButton::Right => {}
            _ => {}
        }
    } else {
        viewer.inputs.mouse_down = false;
    }
    None
}

async fn on_mouse_wheel(
    window: &Window,
    viewer: &mut Viewer,
    delta: &MouseScrollDelta,
    phase: &TouchPhase,
) -> Option<ControlFlow> {
    trace!("Mouse wheel: {:?} {:?}", delta, phase);
    let delta_y = match delta {
        MouseScrollDelta::LineDelta(_x, y) => *y as f64,
        MouseScrollDelta::PixelDelta(delta) => delta.y,
    };
    let size = window.inner_size();
    viewer
        .view
        .zoom(delta_y, (size.width as f64, size.height as f64));
    None
}

async fn on_cursor_moved(
    _window: &Window,
    viewer: &mut Viewer,
    (x1, y1): (f64, f64),
) -> Option<ControlFlow> {
    if let Some((x0, y0)) = viewer.inputs.mouse_pos {
        let (x0, y0) = (
            x0 / viewer.size.width as f64,
            y0 / viewer.size.height as f64,
        );
        let (x1, y1) = (
            x1 / viewer.size.width as f64,
            y1 / viewer.size.height as f64,
        );
        if viewer.inputs.mouse_down {
            let dx = x1 - x0;
            let dy = y1 - y0;
            viewer.view.pan((2.0 * dx, -2.0 * dy));
        }
    }
    viewer.view.cursor = (x1, y1);
    viewer.inputs.mouse_pos = Some((x1, y1));
    None
}

pub async fn on_event<'a>(
    window: &Window,
    event: Event<'a, ()>,
    control_flow: &mut ControlFlow,
    viewer: &mut Viewer,
) {
    match event {
        Event::WindowEvent {
            ref event,
            window_id,
        } if window_id == window.id() => {
            match event {
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                WindowEvent::MouseInput {
                    state: element_state,
                    button,
                    ..
                } => {
                    on_mouse_button(&window, viewer, element_state, button).await;
                }
                WindowEvent::MouseWheel { delta, phase, .. } => {
                    on_mouse_wheel(&window, viewer, delta, phase).await;
                }
                WindowEvent::CursorMoved {
                    position: logical_position,
                    ..
                } => {
                    on_cursor_moved(&window, viewer, (logical_position.x, logical_position.y))
                        .await;
                }
                WindowEvent::KeyboardInput {
                    input:
                        KeyboardInput {
                            state: ElementState::Pressed,
                            virtual_keycode: Some(k),
                            ..
                        },
                    ..
                } => {
                    if let Some(f) = on_key_press(&window, viewer, k).await {
                        *control_flow = f;
                    }
                }
                WindowEvent::Resized(physical_size) => {
                    viewer.resize(*physical_size);
                }
                WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                    // new_inner_size is &mut so w have to dereference it twice
                    viewer.resize(**new_inner_size);
                }
                WindowEvent::AxisMotion { .. } => {
                    // ignore
                }
                _ => {
                    trace!("{:?}", event);
                }
            }
        }
        Event::RedrawRequested(window_id) if window_id == window.id() => {
            let res = viewer.render();
            // extract Some(surface_error) iff err can be downcast to SurfaceError
            let surface_error = res
                .err()
                .and_then(|e| e.downcast::<wgpu::SurfaceError>().ok());
            match surface_error {
                Some(wgpu::SurfaceError::Lost) => viewer.resize(viewer.size),
                Some(wgpu::SurfaceError::OutOfMemory) => {
                    log::warn!("Out of memory, exiting");
                    *control_flow = ControlFlow::Exit
                }
                Some(e) => log::error!("{:?}", e),
                None => {}
            }
        }
        Event::MainEventsCleared => {
            // RedrawRequested will only trigger once, unless we manually
            // request it.
            window.request_redraw();
        }
        Event::RedrawEventsCleared | Event::NewEvents(_) => {}
        Event::DeviceEvent { event, .. } => match event {
            DeviceEvent::MouseMotion { delta: _ } => {
                // ignore
            }
            _ => {
                // ignore
            }
        },
        _ => {
            trace!("{:?}", event);
        }
    }
}
