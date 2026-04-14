mod body_model;
mod camera;
mod entities;
mod gpu;
mod heightmap;
mod hex_overlay;
mod input;

use body_model::{BODY_POINT_COUNT, BodyRenderer, BodyTickData};
use camera::Camera;
use entities::{EntityRenderer, EntityTickData};
use gpu::GpuState;
use heightmap::HeightmapRenderer;
use hex_overlay::HexOverlayRenderer;
use input::InputState;
use js_sys::{ArrayBuffer, Uint8Array};
use simulate_everything_protocol::{
    BodyPointWire, BodyRenderInfo, BodyZone, EntityUpdate, SpectatorEntityInfo, V3Init,
    V3ServerToSpectator, V3Snapshot, V3SnapshotDelta, WoundSeverity, decode,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use web_sys::{BinaryType, MessageEvent, WebSocket};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::web::WindowAttributesExtWebSys;
use winit::window::{Window, WindowId};

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
    body_renderer: BodyRenderer,
    camera: Camera,
    input: InputState,
    live: LiveWorld,
    websocket: Option<WebSocket>,
    last_frame: f64,
}

struct LiveWorld {
    entities: HashMap<u32, SpectatorEntityInfo>,
    body_models: HashMap<u32, BodyRenderInfo>,
    hex_ownership: Vec<Option<u8>>,
    current_tick: u64,
    tick_interval_secs: f32,
    last_tick_received_at_ms: Option<f64>,
}

impl Default for LiveWorld {
    fn default() -> Self {
        Self {
            entities: HashMap::new(),
            body_models: HashMap::new(),
            hex_ownership: Vec::new(),
            current_tick: 0,
            tick_interval_secs: 0.1,
            last_tick_received_at_ms: None,
        }
    }
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

        let attrs = Window::default_attributes().with_canvas(Some(canvas));
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
            let mut camera = Camera::new(size.width as f32, size.height as f32);
            camera.target = glam::Vec3::new(15.0, 0.0, 15.0);
            camera.distance = 120.0;

            let terrain = HeightmapRenderer::new(&gpu, 2, 2, &[0.0; 4], &[0; 4]);
            let hex_overlay = HexOverlayRenderer::new(
                &gpu,
                terrain.camera_bind_group_layout(),
                terrain.heightmap_view(),
                terrain.sampler(),
                2,
                2,
                &[None; 4],
                1,
                1,
            );
            let entity_renderer = EntityRenderer::new(&gpu, terrain.camera_bind_group_layout());
            let body_renderer = BodyRenderer::new(&gpu, terrain.camera_bind_group_layout());

            let now = web_sys::window().unwrap().performance().unwrap().now();

            *state_ref.borrow_mut() = Some(ViewerState {
                gpu,
                terrain,
                hex_overlay,
                entity_renderer,
                body_renderer,
                camera,
                input: InputState::new(),
                live: LiveWorld::default(),
                websocket: None,
                last_frame: now,
            });

            attach_live_socket(state_ref.clone(), win.clone());
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
                let frame_dt = ((now - state.last_frame) / 1000.0) as f32;
                state.last_frame = now;

                state.input.update_camera(&mut state.camera, frame_dt);
                state.terrain.flush_dirty_chunks(&state.gpu.queue);

                let interp_t = match state.live.last_tick_received_at_ms {
                    Some(last_tick_ms) => {
                        ((now - last_tick_ms) as f32 / 1000.0 / state.live.tick_interval_secs)
                            .clamp(0.0, 1.0)
                    }
                    None => 1.0,
                };
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

                state.entity_renderer.interpolate(
                    &mut encoder,
                    &state.gpu.queue,
                    interp_t,
                    camera_pos,
                    viewport_height,
                );
                state
                    .body_renderer
                    .interpolate(&mut encoder, &state.gpu.queue, interp_t);

                state.terrain.render(
                    &mut encoder,
                    &view,
                    &state.gpu.depth_view,
                    &state.gpu.queue,
                    &camera_uniforms,
                    camera_target,
                );
                state.hex_overlay.render(
                    &mut encoder,
                    &view,
                    &state.gpu.depth_view,
                    state.terrain.camera_bind_group(),
                );
                state.entity_renderer.render(
                    &mut encoder,
                    &view,
                    &state.gpu.depth_view,
                    state.terrain.camera_bind_group(),
                );
                state.body_renderer.render(
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

fn attach_live_socket(state_ref: Rc<RefCell<Option<ViewerState>>>, window: Arc<Window>) {
    let location = web_sys::window().unwrap().location();
    let protocol = match location.protocol().unwrap().as_str() {
        "https:" => "wss:",
        _ => "ws:",
    };
    let host = location.host().unwrap();
    let ws_url = format!("{protocol}//{host}/ws/v3/rr");
    let ws = WebSocket::new(&ws_url).expect("Failed to create websocket");
    ws.set_binary_type(BinaryType::Arraybuffer);

    let onopen = Closure::<dyn FnMut(_)>::wrap(Box::new(move |_ev: web_sys::Event| {
        log::info!("viewer websocket connected");
    }));
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    let state_for_msg = state_ref.clone();
    let window_for_msg = window.clone();
    let onmessage = Closure::<dyn FnMut(_)>::wrap(Box::new(move |ev: MessageEvent| {
        let Ok(buf) = ev.data().dyn_into::<ArrayBuffer>() else {
            log::warn!("viewer received non-binary websocket payload");
            return;
        };
        let bytes = Uint8Array::new(&buf).to_vec();
        let msg: V3ServerToSpectator = match decode(&bytes) {
            Ok(msg) => msg,
            Err(err) => {
                log::error!("msgpack decode failed: {err}");
                return;
            }
        };

        let mut state_borrow = state_for_msg.borrow_mut();
        let Some(state) = state_borrow.as_mut() else {
            return;
        };

        match msg {
            V3ServerToSpectator::Init { init, .. } => {
                apply_init(state, init);
            }
            V3ServerToSpectator::Snapshot { snapshot } => {
                apply_snapshot(state, snapshot);
            }
            V3ServerToSpectator::SnapshotDelta { delta } => {
                apply_delta(state, delta);
            }
            V3ServerToSpectator::Config { .. }
            | V3ServerToSpectator::RrStatus(_)
            | V3ServerToSpectator::GameEnd { .. } => {}
        }

        window_for_msg.request_redraw();
    }));
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    let onerror = Closure::<dyn FnMut(_)>::wrap(Box::new(move |_ev: web_sys::Event| {
        log::error!("viewer websocket error");
    }));
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    if let Some(state) = state_ref.borrow_mut().as_mut() {
        state.websocket = Some(ws);
    }
}

fn apply_init(state: &mut ViewerState, init: V3Init) {
    let width = init.width.max(1);
    let height = init.height.max(1);
    let material: Vec<u32> = init
        .material_map
        .iter()
        .map(|value| *value as u32)
        .collect();
    rebuild_terrain_and_overlay(
        state,
        width,
        height,
        &init.height_map,
        &material,
        vec![None; (width * height) as usize],
    );
    state.camera.target = glam::Vec3::new(width as f32 * 0.5, 0.0, height as f32 * 0.5);
    state.camera.distance = (width.max(height) as f32 * 1.8).clamp(60.0, 1200.0);
    state.live.entities.clear();
    state.live.body_models.clear();
    state.live.hex_ownership = vec![None; (width * height) as usize];
    state.live.current_tick = 0;
    state.live.last_tick_received_at_ms = None;
}

fn apply_snapshot(state: &mut ViewerState, snapshot: V3Snapshot) {
    state.live.entities = snapshot.entities.into_iter().map(|e| (e.id, e)).collect();
    state.live.body_models = snapshot
        .body_models
        .into_iter()
        .map(|b| (b.entity_id, b))
        .collect();
    if state.live.hex_ownership != snapshot.hex_ownership {
        state.live.hex_ownership = snapshot.hex_ownership.clone();
        rebuild_overlay(state, snapshot.hex_ownership);
    }
    mark_tick_received(&mut state.live, snapshot.tick);
    upload_live_state(state);
}

fn apply_delta(state: &mut ViewerState, delta: V3SnapshotDelta) {
    for entity in delta.entities_appeared {
        state.live.entities.insert(entity.id, entity);
    }
    for update in delta.entities_updated {
        apply_entity_update(&mut state.live.entities, update);
    }
    for entity_id in delta.entities_removed {
        state.live.entities.remove(&entity_id);
    }

    for body in delta.body_models_appeared {
        state.live.body_models.insert(body.entity_id, body);
    }
    for body in delta.body_models_updated {
        state.live.body_models.insert(body.entity_id, body);
    }
    for entity_id in delta.body_models_removed {
        state.live.body_models.remove(&entity_id);
    }

    if !delta.hex_changes.is_empty() {
        for change in delta.hex_changes {
            if let Some(owner) = change.owner {
                if let Some(slot) = state.live.hex_ownership.get_mut(change.index as usize) {
                    *slot = Some(owner);
                }
            }
        }
        rebuild_overlay(state, state.live.hex_ownership.clone());
    }

    mark_tick_received(&mut state.live, delta.tick);
    upload_live_state(state);
}

fn mark_tick_received(live: &mut LiveWorld, tick: u64) {
    let now = web_sys::window().unwrap().performance().unwrap().now();
    if let Some(prev_now) = live.last_tick_received_at_ms {
        live.tick_interval_secs = ((now - prev_now) as f32 / 1000.0).clamp(0.016, 1.0);
    }
    live.last_tick_received_at_ms = Some(now);
    live.current_tick = tick;
}

fn rebuild_terrain_and_overlay(
    state: &mut ViewerState,
    width: u32,
    height: u32,
    height_map: &[f32],
    material_map: &[u32],
    hex_ownership: Vec<Option<u8>>,
) {
    state.terrain = HeightmapRenderer::new(&state.gpu, width, height, height_map, material_map);
    state.hex_overlay = HexOverlayRenderer::new(
        &state.gpu,
        state.terrain.camera_bind_group_layout(),
        state.terrain.heightmap_view(),
        state.terrain.sampler(),
        width,
        height,
        &hex_ownership,
        width,
        height,
    );
    state.entity_renderer =
        EntityRenderer::new(&state.gpu, state.terrain.camera_bind_group_layout());
    state.body_renderer = BodyRenderer::new(&state.gpu, state.terrain.camera_bind_group_layout());
}

fn rebuild_overlay(state: &mut ViewerState, hex_ownership: Vec<Option<u8>>) {
    let width = state.terrain.map_width();
    let height = state.terrain.map_height();
    state.hex_overlay = HexOverlayRenderer::new(
        &state.gpu,
        state.terrain.camera_bind_group_layout(),
        state.terrain.heightmap_view(),
        state.terrain.sampler(),
        width,
        height,
        &hex_ownership,
        width,
        height,
    );
}

fn upload_live_state(state: &mut ViewerState) {
    let mut entity_ids: Vec<u32> = state.live.entities.keys().copied().collect();
    entity_ids.sort_unstable();
    let entities: Vec<EntityTickData> = entity_ids
        .iter()
        .filter_map(|id| state.live.entities.get(id))
        .map(|entity| {
            let pos = sim_to_view(entity.x, entity.y, entity.z);
            EntityTickData {
                pos,
                facing: entity.facing.unwrap_or(0.0),
                owner: entity.owner.unwrap_or(0) as u32,
                entity_kind: match entity.entity_kind {
                    simulate_everything_protocol::EntityKind::Person => 0,
                    simulate_everything_protocol::EntityKind::Structure => 1,
                },
                health_frac: entity.blood.unwrap_or(1.0).clamp(0.0, 1.0),
                stamina_frac: entity.stamina.unwrap_or(1.0).clamp(0.0, 1.0),
                flags: u32::from(state.live.body_models.contains_key(&entity.id)),
                _pad: [0.0; 3],
            }
        })
        .collect();
    state.entity_renderer.push_tick(&state.gpu.queue, &entities);

    let mut body_ids: Vec<u32> = state.live.body_models.keys().copied().collect();
    body_ids.sort_unstable();
    let bodies: Vec<BodyTickData> = body_ids
        .iter()
        .filter_map(|id| {
            let body = state.live.body_models.get(id)?;
            let entity = state.live.entities.get(id);
            Some(BodyTickData {
                points: convert_points(&body.points),
                weapon_a: body
                    .weapon
                    .map(convert_capsule_start)
                    .unwrap_or([0.0, 0.0, 0.0, 0.0]),
                weapon_b: body
                    .weapon
                    .map(convert_capsule_end)
                    .unwrap_or([0.0, 0.0, 0.0, 0.0]),
                shield_center: body
                    .shield
                    .map(convert_disc_center)
                    .unwrap_or([0.0, 0.0, 0.0, 0.0]),
                shield_normal: body
                    .shield
                    .map(convert_disc_normal)
                    .unwrap_or([0.0, 0.0, 0.0, 0.0]),
                owner: entity.and_then(|e| e.owner).unwrap_or(0) as u32,
                wound_mask: entity.map(wound_mask).unwrap_or(0),
                _pad: [0; 2],
            })
        })
        .collect();
    state.body_renderer.push_tick(&state.gpu.queue, &bodies);
}

fn sim_to_view(x: f32, y: f32, z: f32) -> [f32; 3] {
    [x, z, y]
}

fn convert_points(points: &[BodyPointWire; BODY_POINT_COUNT]) -> [[f32; 4]; BODY_POINT_COUNT] {
    std::array::from_fn(|idx| {
        let p = points[idx];
        let pos = sim_to_view(p.x, p.y, p.z);
        [pos[0], pos[1], pos[2], 0.0]
    })
}

fn convert_capsule_start(capsule: simulate_everything_protocol::CapsuleWire) -> [f32; 4] {
    let pos = sim_to_view(capsule.a[0], capsule.a[1], capsule.a[2]);
    [pos[0], pos[1], pos[2], capsule.radius]
}

fn convert_capsule_end(capsule: simulate_everything_protocol::CapsuleWire) -> [f32; 4] {
    let pos = sim_to_view(capsule.b[0], capsule.b[1], capsule.b[2]);
    [pos[0], pos[1], pos[2], 1.0]
}

fn convert_disc_center(disc: simulate_everything_protocol::DiscWire) -> [f32; 4] {
    let pos = sim_to_view(disc.center[0], disc.center[1], disc.center[2]);
    [pos[0], pos[1], pos[2], disc.radius]
}

fn convert_disc_normal(disc: simulate_everything_protocol::DiscWire) -> [f32; 4] {
    let normal = sim_to_view(disc.normal[0], disc.normal[1], disc.normal[2]);
    [normal[0], normal[1], normal[2], 1.0]
}

fn severity_bits(severity: WoundSeverity) -> u32 {
    match severity {
        WoundSeverity::Light => 1,
        WoundSeverity::Serious => 2,
        WoundSeverity::Critical => 3,
    }
}

fn zone_shift(zone: BodyZone) -> u32 {
    match zone {
        BodyZone::Head => 0,
        BodyZone::Torso => 3,
        BodyZone::LeftArm => 6,
        BodyZone::RightArm => 9,
        BodyZone::Legs => 12,
    }
}

fn wound_mask(entity: &SpectatorEntityInfo) -> u32 {
    entity.wounds.iter().fold(0u32, |mask, (zone, severity)| {
        mask | (severity_bits(*severity) << zone_shift(*zone))
    })
}

fn apply_entity_update(entities: &mut HashMap<u32, SpectatorEntityInfo>, update: EntityUpdate) {
    let Some(entity) = entities.get_mut(&update.id) else {
        return;
    };

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
    if let Some(wounds) = update.wounds {
        entity.wounds = wounds;
    }
    if let Some(weapon_type) = update.weapon_type {
        entity.weapon_type = Some(weapon_type);
    }
    if let Some(armor_type) = update.armor_type {
        entity.armor_type = Some(armor_type);
    }
    if let Some(contains_count) = update.contains_count {
        entity.contains_count = contains_count;
    }
    if let Some(stack_id) = update.stack_id {
        entity.stack_id = stack_id;
    }
    if let Some(current_task) = update.current_task {
        entity.current_task = current_task;
    }
    if let Some(attack_phase) = update.attack_phase {
        entity.attack_phase = attack_phase;
    }
    if let Some(attack_motion) = update.attack_motion {
        entity.attack_motion = attack_motion;
    }
    if let Some(weapon_angle) = update.weapon_angle {
        entity.weapon_angle = weapon_angle;
    }
    if let Some(attack_progress) = update.attack_progress {
        entity.attack_progress = attack_progress;
    }
}
