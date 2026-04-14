mod camera;
mod entities;
mod gpu;
mod heightmap;
mod hex_overlay;
mod input;

use camera::Camera;
use entities::{EntityRenderer, EntityTickData};
use gpu::GpuState;
use heightmap::HeightmapRenderer;
use hex_overlay::HexOverlayRenderer;
use input::InputState;
use simulate_everything_protocol::{
    EntityKind, EntityUpdate, SpectatorEntityInfo, TerrainPatch, V3Init, V3ServerToSpectator,
    V3Snapshot, V3SnapshotDelta, decode,
};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsCast, closure::Closure};
#[cfg(target_arch = "wasm32")]
use web_sys::{BinaryType, MessageEvent, WebSocket};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::ActiveEventLoop;
#[cfg(target_arch = "wasm32")]
use winit::event_loop::EventLoop;
#[cfg(target_arch = "wasm32")]
use winit::platform::web::WindowAttributesExtWebSys;
use winit::window::{Window, WindowId};

/// WASM entry point. Called by Trunk-generated JS glue.
#[cfg(target_arch = "wasm32")]
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
    hex_overlay: HexOverlayRenderer,
    entity_renderer: EntityRenderer,
    camera: Camera,
    input: InputState,
    last_frame: f64,
    grid_width: u32,
    grid_height: u32,
    hex_ownership: Vec<Option<u8>>,
    entities: HashMap<u32, SpectatorEntityInfo>,
}

impl ApplicationHandler for ViewerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.init_started {
            return;
        }
        self.init_started = true;

        let doc = web_sys::window().unwrap().document().unwrap();
        let root = doc.get_element_by_id("viewer-root").unwrap();
        let canvas: web_sys::HtmlCanvasElement =
            doc.create_element("canvas").unwrap().dyn_into().unwrap();
        root.prepend_with_node_1(&canvas).unwrap();

        #[cfg(target_arch = "wasm32")]
        let attrs = Window::default_attributes().with_canvas(Some(canvas));
        #[cfg(not(target_arch = "wasm32"))]
        let attrs = Window::default_attributes();
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("Failed to create window"),
        );
        self.window = Some(window.clone());

        let state_ref = self.state.clone();
        let win = window.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let gpu = GpuState::new(win.clone()).await;

            let size = win.inner_size();
            let camera = Camera::new(size.width as f32, size.height as f32);

            // Generate demo heightmap
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
                        2
                    } else if h > 15.0 {
                        1
                    } else if h < -5.0 {
                        3
                    } else {
                        0
                    };
                }
            }

            let terrain = HeightmapRenderer::new(
                &gpu,
                map_size,
                map_size,
                0.0,
                0.0,
                1.0,
                &height_data,
                &material_data,
            );

            // Demo hex ownership
            let hex_w = 7u32;
            let hex_h = 7u32;
            let hex_ownership: Vec<Option<u8>> = (0..(hex_w * hex_h))
                .map(|i| {
                    let q = (i % hex_w) as i32;
                    let r = (i / hex_w) as i32;
                    let cq = q as f32 - hex_w as f32 / 2.0;
                    let cr = r as f32 - hex_h as f32 / 2.0;
                    let dist = (cq * cq + cr * cr).sqrt();
                    if dist < 2.0 {
                        Some(0)
                    } else if dist < 3.5 {
                        Some(1)
                    } else {
                        None
                    }
                })
                .collect();

            let hex_overlay = HexOverlayRenderer::new(
                &gpu,
                terrain.camera_bind_group_layout(),
                terrain.heightmap_view(),
                terrain.sampler(),
                0.0,
                0.0,
                1.0,
                map_size,
                map_size,
                &hex_ownership,
                hex_w,
                hex_h,
            );

            // Entity renderer
            let mut entity_renderer = EntityRenderer::new(&gpu, terrain.camera_bind_group_layout());

            // Demo entities: scatter 500 entities around the map center
            let demo_entities: Vec<EntityTickData> = (0..500)
                .map(|i| {
                    let angle = i as f32 * 0.1;
                    let radius = 50.0 + (i as f32 * 0.7) % 200.0;
                    let x = 512.0 + radius * angle.cos();
                    let z = 512.0 + radius * angle.sin();
                    // Look up height at this position
                    let tx = (x as u32).min(map_size - 1);
                    let tz = (z as u32).min(map_size - 1);
                    let y = height_data[(tz * map_size + tx) as usize];

                    EntityTickData {
                        pos: [x, y, z],
                        facing: angle,
                        owner: (i % 4) as u32,
                        entity_kind: 0, // person
                        health_frac: 1.0 - (i as f32 * 0.001),
                        stamina_frac: 0.8,
                        flags: 0,
                        _pad: [0.0; 3],
                    }
                })
                .collect();

            entity_renderer.push_tick(&gpu.queue, &demo_entities);

            let now = web_sys::window().unwrap().performance().unwrap().now();

            *state_ref.borrow_mut() = Some(ViewerState {
                gpu,
                terrain,
                hex_overlay,
                entity_renderer,
                camera,
                input: InputState::new(),
                last_frame: now,
                grid_width: hex_w,
                grid_height: hex_h,
                hex_ownership,
                entities: HashMap::new(),
            });

            #[cfg(target_arch = "wasm32")]
            connect_rr_socket(state_ref.clone(), win.clone());

            log::info!("GPU initialized, terrain + hex overlay + entities ready");
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
                let now = web_sys::window().unwrap().performance().unwrap().now();
                let dt = ((now - state.last_frame) / 1000.0) as f32;
                state.last_frame = now;

                state.input.update_camera(&mut state.camera, dt);
                state.terrain.flush_dirty_chunks(&state.gpu.queue);

                let camera_uniforms = state.camera.uniforms();
                let camera_target = state.camera.target.to_array();
                let camera_pos = state.camera.eye().to_array();
                let viewport_height = state.camera.height;

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
                let mut encoder =
                    state
                        .gpu
                        .device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("frame"),
                        });

                // Compute: interpolate entities + assign LOD
                state.entity_renderer.interpolate(
                    &mut encoder,
                    &state.gpu.queue,
                    1.0, // t=1.0 (no interpolation yet — needs WS tick stream)
                    camera_pos,
                    viewport_height,
                );

                // Pass 1: terrain
                state.terrain.render(
                    &mut encoder,
                    &view,
                    &state.gpu.depth_view,
                    &state.gpu.queue,
                    &camera_uniforms,
                    camera_target,
                );

                // Pass 2: hex overlay
                state.hex_overlay.render(
                    &mut encoder,
                    &view,
                    &state.gpu.depth_view,
                    state.terrain.camera_bind_group(),
                );

                // Pass 3: entities
                state.entity_renderer.render(
                    &mut encoder,
                    &view,
                    &state.gpu.depth_view,
                    state.terrain.camera_bind_group(),
                );

                state.gpu.queue.submit(std::iter::once(encoder.finish()));
                frame.present();

                drop(state_borrow);
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

fn viewer_entity(entity: &SpectatorEntityInfo) -> EntityTickData {
    EntityTickData {
        pos: [entity.x, entity.z, entity.y],
        facing: entity.facing.unwrap_or(0.0),
        owner: entity.owner.unwrap_or(0) as u32,
        entity_kind: match entity.entity_kind {
            EntityKind::Person => 0,
            EntityKind::Structure => 1,
        },
        health_frac: entity.blood.unwrap_or(1.0),
        stamina_frac: entity.stamina.unwrap_or(1.0),
        flags: 0,
        _pad: [0.0; 3],
    }
}

fn push_entities_to_gpu(state: &mut ViewerState) {
    let entities: Vec<EntityTickData> = state.entities.values().map(viewer_entity).collect();
    state.entity_renderer.push_tick(&state.gpu.queue, &entities);
}

fn apply_entity_update(entity: &mut SpectatorEntityInfo, update: &EntityUpdate) {
    if let Some(x) = update.x {
        entity.x = x;
    }
    if let Some(y) = update.y {
        entity.y = y;
    }
    if let Some(z) = update.z {
        entity.z = z;
    }
    if let Some(hex_q) = update.hex_q {
        entity.hex_q = hex_q;
    }
    if let Some(hex_r) = update.hex_r {
        entity.hex_r = hex_r;
    }
    if let Some(facing) = update.facing {
        entity.facing = Some(facing);
    }
    if let Some(blood) = update.blood {
        entity.blood = Some(blood);
    }
    if let Some(stamina) = update.stamina {
        entity.stamina = Some(stamina);
    }
    if let Some(wounds) = &update.wounds {
        entity.wounds = wounds.clone();
    }
    if let Some(contains_count) = update.contains_count {
        entity.contains_count = contains_count;
    }
}

fn apply_terrain_patch(state: &mut ViewerState, patch: &TerrainPatch) {
    state.terrain.mutate_terrain_patch(
        patch.x,
        patch.y,
        patch.width,
        patch.height,
        &patch.heights,
        &patch.materials,
    );
}

fn handle_init_message(state: &mut ViewerState, init: &V3Init) {
    state.terrain = HeightmapRenderer::new(
        &state.gpu,
        init.terrain_raster.width,
        init.terrain_raster.height,
        init.terrain_raster.origin_x,
        init.terrain_raster.origin_y,
        init.terrain_raster.cell_size,
        &init.terrain_raster.heights,
        &init.terrain_raster.materials,
    );
    state.hex_ownership = vec![None; (init.width * init.height) as usize];
    state.grid_width = init.width;
    state.grid_height = init.height;
    state.hex_overlay = HexOverlayRenderer::new(
        &state.gpu,
        state.terrain.camera_bind_group_layout(),
        state.terrain.heightmap_view(),
        state.terrain.sampler(),
        init.terrain_raster.origin_x,
        init.terrain_raster.origin_y,
        init.terrain_raster.cell_size,
        init.terrain_raster.width,
        init.terrain_raster.height,
        &state.hex_ownership,
        init.width,
        init.height,
    );
    state.camera.target = glam::Vec3::new(
        init.terrain_raster.origin_x
            + (init.terrain_raster.width as f32 * init.terrain_raster.cell_size) * 0.5,
        0.0,
        init.terrain_raster.origin_y
            + (init.terrain_raster.height as f32 * init.terrain_raster.cell_size) * 0.5,
    );
}

fn handle_snapshot_message(state: &mut ViewerState, snapshot: &V3Snapshot) {
    state.entities = snapshot
        .entities
        .iter()
        .cloned()
        .map(|entity| (entity.id, entity))
        .collect();
    state.hex_ownership = snapshot.hex_ownership.clone();
    state.hex_overlay.update_ownership(
        &state.gpu.device,
        &state.hex_ownership,
        state.grid_width,
        state.grid_height,
    );
    push_entities_to_gpu(state);
}

fn handle_delta_message(state: &mut ViewerState, delta: &V3SnapshotDelta) {
    for entity in &delta.entities_appeared {
        state.entities.insert(entity.id, entity.clone());
    }
    for update in &delta.entities_updated {
        if let Some(entity) = state.entities.get_mut(&update.id) {
            apply_entity_update(entity, update);
        }
    }
    for removed in &delta.entities_removed {
        state.entities.remove(removed);
    }
    if !delta.hex_changes.is_empty() {
        for change in &delta.hex_changes {
            if let Some(owner) = change.owner
                && let Some(cell) = state.hex_ownership.get_mut(change.index as usize)
            {
                *cell = Some(owner);
            } else if change.owner.is_none()
                && let Some(cell) = state.hex_ownership.get_mut(change.index as usize)
            {
                *cell = None;
            }
        }
        state.hex_overlay.update_ownership(
            &state.gpu.device,
            &state.hex_ownership,
            state.grid_width,
            state.grid_height,
        );
    }
    for patch in &delta.terrain_patches {
        apply_terrain_patch(state, patch);
    }
    push_entities_to_gpu(state);
}

#[cfg(target_arch = "wasm32")]
fn connect_rr_socket(state_ref: Rc<RefCell<Option<ViewerState>>>, win: Arc<Window>) {
    let location = web_sys::window().unwrap().location();
    let protocol = if location.protocol().unwrap_or_default() == "https:" {
        "wss:"
    } else {
        "ws:"
    };
    let host = location.host().unwrap_or_default();
    let url = format!("{protocol}//{host}/ws/v3/rr");
    let socket = WebSocket::new(&url).expect("ws connection");
    socket.set_binary_type(BinaryType::Arraybuffer);

    let state_for_msg = state_ref.clone();
    let win_for_msg = win.clone();
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let event_data = event.data();
        let Some(data) = event_data.dyn_ref::<js_sys::ArrayBuffer>() else {
            return;
        };
        let bytes = js_sys::Uint8Array::new(data).to_vec();
        let Ok(message) = decode::<V3ServerToSpectator>(&bytes) else {
            return;
        };
        let mut state_borrow = state_for_msg.borrow_mut();
        let Some(state) = state_borrow.as_mut() else {
            return;
        };
        match message {
            V3ServerToSpectator::Init { init, .. } => handle_init_message(state, &init),
            V3ServerToSpectator::Snapshot { snapshot } => handle_snapshot_message(state, &snapshot),
            V3ServerToSpectator::SnapshotDelta { delta } => handle_delta_message(state, &delta),
            V3ServerToSpectator::GameEnd { .. }
            | V3ServerToSpectator::Config { .. }
            | V3ServerToSpectator::RrStatus(_) => {}
        }
        win_for_msg.request_redraw();
    });
    socket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();
}
