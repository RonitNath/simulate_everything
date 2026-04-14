mod camera;
mod gpu;
mod heightmap;
mod input;

use camera::Camera;
use gpu::GpuState;
use heightmap::HeightmapRenderer;
use input::InputState;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::web::WindowAttributesExtWebSys;
use winit::window::{Window, WindowId};

/// WASM entry point. Called by Trunk-generated JS glue.
#[wasm_bindgen(start)]
pub fn start() {
    wasm_logger::init(wasm_logger::Config::default());
    log::info!("simulate_everything viewer starting");

    let event_loop = EventLoop::new().expect("Failed to create event loop");
    let mut app = ViewerApp {
        state: Rc::new(RefCell::new(None)),
        window: None,
        init_started: false,
    };
    event_loop.run_app(&mut app).expect("Event loop failed");
}

struct ViewerApp {
    state: Rc<RefCell<Option<ViewerState>>>,
    window: Option<Arc<Window>>,
    init_started: bool,
}

struct ViewerState {
    gpu: GpuState,
    terrain: HeightmapRenderer,
    camera: Camera,
    input: InputState,
    last_frame: f64,
}

impl ApplicationHandler for ViewerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.init_started {
            return;
        }
        self.init_started = true;

        // Create canvas in the DOM
        let doc = web_sys::window().unwrap().document().unwrap();
        let root = doc.get_element_by_id("viewer-root").unwrap();
        let canvas: web_sys::HtmlCanvasElement = doc
            .create_element("canvas")
            .unwrap()
            .dyn_into()
            .unwrap();
        // Insert canvas before the UI overlay div
        root.prepend_with_node_1(&canvas).unwrap();

        let attrs = Window::default_attributes().with_canvas(Some(canvas));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("Failed to create window"),
        );
        self.window = Some(window.clone());

        // Kick off async GPU initialization
        let state_ref = self.state.clone();
        let win = window.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let gpu = GpuState::new(win.clone()).await;

            let size = win.inner_size();
            let camera = Camera::new(size.width as f32, size.height as f32);

            // Generate demo heightmap (rolling hills)
            let map_size = 1024u32;
            let n = (map_size * map_size) as usize;
            let mut height_data = vec![0.0f32; n];
            let mut material_data = vec![0u32; n];

            for z in 0..map_size {
                for x in 0..map_size {
                    let fx = x as f32;
                    let fz = z as f32;
                    let dist = ((fx - 512.0).powi(2) + (fz - 512.0).powi(2)).sqrt();
                    let h = 20.0 * (fx * 0.01).sin() * (fz * 0.01).cos()
                        + 8.0 * (fx * 0.03 + 1.0).sin() * (fz * 0.025).cos()
                        + 3.0 * (fx * 0.08).sin() * (fz * 0.07 + 2.0).cos()
                        + 40.0 * (-dist / 300.0).exp();

                    let idx = (z * map_size + x) as usize;
                    height_data[idx] = h;
                    material_data[idx] = if h > 30.0 {
                        2 // rock
                    } else if h > 15.0 {
                        1 // dirt
                    } else if h < -5.0 {
                        3 // sand
                    } else {
                        0 // grass
                    };
                }
            }

            let terrain =
                HeightmapRenderer::new(&gpu, map_size, map_size, &height_data, &material_data);

            let now = web_sys::window()
                .unwrap()
                .performance()
                .unwrap()
                .now();

            *state_ref.borrow_mut() = Some(ViewerState {
                gpu,
                terrain,
                camera,
                input: InputState::new(),
                last_frame: now,
            });

            log::info!("GPU initialized, terrain ready");
            win.request_redraw();
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let mut state_borrow = self.state.borrow_mut();
        let Some(state) = state_borrow.as_mut() else {
            return;
        };

        match event {
            WindowEvent::Resized(size) => {
                state.gpu.resize(size.width, size.height);
                state.camera.resize(size.width as f32, size.height as f32);
            }
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. } => match event.state {
                winit::event::ElementState::Pressed => state.input.key_down(event.physical_key),
                winit::event::ElementState::Released => state.input.key_up(event.physical_key),
            },
            WindowEvent::MouseInput {
                button,
                state: btn_state,
                ..
            } => {
                state.input.mouse_button(button, btn_state);
            }
            WindowEvent::CursorMoved { position, .. } => {
                state
                    .input
                    .mouse_move(position.x, position.y, &mut state.camera);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                state.input.mouse_wheel(delta, &mut state.camera);
            }
            WindowEvent::RedrawRequested => {
                let now = web_sys::window()
                    .unwrap()
                    .performance()
                    .unwrap()
                    .now();
                let dt = ((now - state.last_frame) / 1000.0) as f32;
                state.last_frame = now;

                // Update camera from held keys
                state.input.update_camera(&mut state.camera, dt);

                // Render
                let camera_uniforms = state.camera.uniforms();
                let camera_target = state.camera.target.to_array();

                let frame = match state.gpu.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame) => frame,
                    other => {
                        log::warn!("Surface error: {:?}", other);
                        return;
                    }
                };

                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder = state.gpu.device.create_command_encoder(
                    &wgpu::CommandEncoderDescriptor {
                        label: Some("frame"),
                    },
                );

                state.terrain.render(
                    &mut encoder,
                    &view,
                    &state.gpu.depth_view,
                    &state.gpu.queue,
                    &camera_uniforms,
                    camera_target,
                );

                state.gpu.queue.submit(std::iter::once(encoder.finish()));
                frame.present();

                // Request next frame
                if let Some(win) = &self.window {
                    win.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        _event: DeviceEvent,
    ) {
    }
}
