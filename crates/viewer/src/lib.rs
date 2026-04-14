mod body_model;
mod camera;
mod entities;
mod gpu;
mod heightmap;
mod hex_overlay;
mod input;
mod overlay;

use body_model::{BODY_POINT_COUNT, BodyRenderer, BodyTickData};
use camera::Camera;
use entities::{EntityRenderer, EntityTickData};
use gpu::GpuState;
use heightmap::HeightmapRenderer;
use hex_overlay::HexOverlayRenderer;
use input::InputState;
use js_sys::{ArrayBuffer, Object, Reflect, Uint8Array};
use overlay::OverlayUi;
use simulate_everything_protocol::{
    BodyPointWire, BodyRenderInfo, BodyZone, EntityKind, EntityUpdate, SpectatorEntityInfo, V3Init,
    V3ServerToSpectator, V3Snapshot, V3SnapshotDelta, WoundSeverity, decode,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use web_sys::{BinaryType, MessageEvent, Url, UrlSearchParams, WebSocket};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, MouseButton, WindowEvent};
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
    overlay: OverlayUi,
    live: LiveWorld,
    websocket: Option<WebSocket>,
    last_frame: f64,
    selected_entity_id: Option<u32>,
    cursor_pos: Option<(f32, f32)>,
}

struct LiveWorld {
    entities: HashMap<u32, SpectatorEntityInfo>,
    body_models: HashMap<u32, BodyRenderInfo>,
    hex_ownership: Vec<Option<u8>>,
    hex_grid_width: u32,
    hex_grid_height: u32,
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
            hex_grid_width: 0,
            hex_grid_height: 0,
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
            let gpu = match GpuState::new(win.clone()).await {
                Ok(gpu) => gpu,
                Err(msg) => {
                    log::error!("{msg}");
                    let doc = web_sys::window().unwrap().document().unwrap();
                    if let Some(root) = doc.get_element_by_id("viewer-root") {
                        root.set_inner_html(&format!(
                            "<div style=\"display:flex;align-items:center;justify-content:center;\
                             height:100%;color:#f3efe7;font:500 16px/1.5 ui-sans-serif,system-ui,sans-serif;\
                             text-align:center;padding:2em\">\
                             <div><p style=\"font-size:20px;margin-bottom:0.5em\">WebGPU not available</p>\
                             <p style=\"opacity:0.7;max-width:36em\">{msg}</p></div></div>"
                        ));
                    }
                    return;
                }
            };
            let size = win.inner_size();
            let mut camera = Camera::new(size.width as f32, size.height as f32);
            camera.target = glam::Vec3::new(15.0, 0.0, 15.0);
            camera.distance = 120.0;

            let terrain = HeightmapRenderer::new(&gpu, 2, 2, 0.0, 0.0, 1.0, &[0.0; 4], &[0; 4]);
            let hex_overlay = HexOverlayRenderer::new(
                &gpu,
                terrain.camera_bind_group_layout(),
                terrain.heightmap_view(),
                terrain.sampler(),
                0.0,
                0.0,
                1.0,
                2,
                2,
                &[None; 4],
                1,
                1,
            );
            let entity_renderer = EntityRenderer::new(&gpu, terrain.camera_bind_group_layout());
            let body_renderer = BodyRenderer::new(&gpu, terrain.camera_bind_group_layout());
            let overlay = OverlayUi::new();

            let now = web_sys::window().unwrap().performance().unwrap().now();

            *state_ref.borrow_mut() = Some(ViewerState {
                gpu,
                terrain,
                hex_overlay,
                entity_renderer,
                body_renderer,
                camera,
                input: InputState::new(),
                overlay,
                live: LiveWorld::default(),
                websocket: None,
                last_frame: now,
                selected_entity_id: None,
                cursor_pos: None,
            });

            attach_live_socket(state_ref.clone(), win.clone());
            publish_selection(None);
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
                if button == MouseButton::Left && btn_state == winit::event::ElementState::Pressed {
                    let selected =
                        pick_entity(&state.camera, &state.live.entities, state.cursor_pos);
                    if selected != state.selected_entity_id {
                        state.selected_entity_id = selected;
                        publish_selection(selected);
                    }
                }
                state.input.mouse_button(button, btn_state);
            }
            WindowEvent::CursorMoved { position, .. } => {
                state.cursor_pos = Some((position.x as f32, position.y as f32));
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
                state.overlay.update(
                    &state.camera,
                    &state.live.entities,
                    state.selected_entity_id,
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
    let ws_url = resolve_ws_url();
    log::info!("viewer connecting to {ws_url}");
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

fn resolve_ws_url() -> String {
    let location = web_sys::window().unwrap().location();
    let current_href = location.href().unwrap_or_else(|_| String::new());
    let current_protocol = location.protocol().unwrap_or_else(|_| "http:".to_string());
    let current_host = location.host().unwrap_or_else(|_| "127.0.0.1".to_string());

    let search = location.search().unwrap_or_default();
    let params = match UrlSearchParams::new_with_str(&search) {
        Ok(params) => params,
        Err(_) => {
            log::warn!("failed to parse query parameters from location search: {search}");
            return format!(
                "{}//{current_host}/ws/v3/rr",
                http_to_ws_scheme(&current_protocol)
            );
        }
    };

    if let Some(explicit_ws) = params.get("ws") {
        if let Some(url) = normalize_ws_override(&explicit_ws, &current_href) {
            return url;
        }
        log::warn!("ignoring invalid viewer ws override: {explicit_ws}");
    }

    if let Some(server_origin) = params.get("server") {
        if let Some(url) = derive_ws_from_server(&server_origin, &current_href) {
            return url;
        }
        log::warn!("ignoring invalid viewer server override: {server_origin}");
    }

    format!(
        "{}//{current_host}/ws/v3/rr",
        http_to_ws_scheme(&current_protocol)
    )
}

fn normalize_ws_override(candidate: &str, current_href: &str) -> Option<String> {
    let url = resolve_url(candidate, current_href)?;
    match url.protocol().as_str() {
        "ws:" | "wss:" => Some(url.href()),
        _ => None,
    }
}

fn derive_ws_from_server(candidate: &str, current_href: &str) -> Option<String> {
    let url = resolve_url(candidate, current_href)?;
    let ws_scheme = match url.protocol().as_str() {
        "http:" => "ws:",
        "https:" => "wss:",
        _ => return None,
    };
    Some(format!("{ws_scheme}//{}/ws/v3/rr", url.host()))
}

fn resolve_url(candidate: &str, current_href: &str) -> Option<Url> {
    Url::new(candidate)
        .ok()
        .or_else(|| Url::new_with_base(candidate, current_href).ok())
}

fn http_to_ws_scheme(protocol: &str) -> &'static str {
    match protocol {
        "https:" => "wss:",
        _ => "ws:",
    }
}

fn publish_selection(entity_id: Option<u32>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(parent) = window.parent().ok().flatten() else {
        return;
    };
    if parent == window {
        return;
    }

    let message = Object::new();
    let _ = Reflect::set(
        &message,
        &JsValue::from_str("type"),
        &JsValue::from_str("viewer-select-entity"),
    );
    let _ = Reflect::set(
        &message,
        &JsValue::from_str("entityId"),
        &entity_id.map_or(JsValue::NULL, |id| JsValue::from_f64(id as f64)),
    );
    let _ = parent.post_message(&message, "*");
}

fn apply_init(state: &mut ViewerState, init: V3Init) {
    let width = init.width.max(1);
    let height = init.height.max(1);
    let raster = &init.terrain_raster;
    let rw = raster.width.max(1);
    let rh = raster.height.max(1);
    log::info!(
        "apply_init: grid {}x{}, raster {}x{} origin=({},{}) cell={}",
        width,
        height,
        rw,
        rh,
        raster.origin_x,
        raster.origin_y,
        raster.cell_size,
    );
    rebuild_terrain_and_overlay(
        state,
        rw,
        rh,
        raster.origin_x,
        raster.origin_y,
        raster.cell_size,
        width,
        height,
        &raster.heights,
        &raster.materials,
        vec![None; (width * height) as usize],
    );
    // Center camera on the raster extent, not the hex grid
    let raster_cx = raster.origin_x + (rw as f32 * raster.cell_size) * 0.5;
    let raster_cz = raster.origin_y + (rh as f32 * raster.cell_size) * 0.5;
    let raster_extent = (rw.max(rh) as f32 * raster.cell_size).max(width.max(height) as f32);
    state.camera.target = glam::Vec3::new(raster_cx, 0.0, raster_cz);
    state.camera.distance = (raster_extent * 1.2).clamp(60.0, 12000.0);
    state.live.entities.clear();
    state.live.body_models.clear();
    state.live.hex_ownership = vec![None; (width * height) as usize];
    state.live.hex_grid_width = width;
    state.live.hex_grid_height = height;
    state.live.current_tick = 0;
    state.live.last_tick_received_at_ms = None;
    if state.selected_entity_id.take().is_some() {
        publish_selection(None);
    }
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
    sync_selection(state);
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

    for patch in &delta.terrain_patches {
        state.terrain.mutate_terrain_patch(
            patch.x,
            patch.y,
            patch.width,
            patch.height,
            &patch.heights,
            &patch.materials,
        );
    }

    mark_tick_received(&mut state.live, delta.tick);
    sync_selection(state);
    upload_live_state(state);
}

fn sync_selection(state: &mut ViewerState) {
    let Some(selected) = state.selected_entity_id else {
        return;
    };
    if state.live.entities.contains_key(&selected) {
        return;
    }
    state.selected_entity_id = None;
    publish_selection(None);
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
    raster_width: u32,
    raster_height: u32,
    origin_x: f32,
    origin_z: f32,
    cell_size: f32,
    grid_width: u32,
    grid_height: u32,
    height_map: &[f32],
    material_map: &[u32],
    hex_ownership: Vec<Option<u8>>,
) {
    state.terrain = HeightmapRenderer::new(
        &state.gpu,
        raster_width,
        raster_height,
        origin_x,
        origin_z,
        cell_size,
        height_map,
        material_map,
    );
    state.hex_overlay = HexOverlayRenderer::new(
        &state.gpu,
        state.terrain.camera_bind_group_layout(),
        state.terrain.heightmap_view(),
        state.terrain.sampler(),
        origin_x,
        origin_z,
        cell_size,
        raster_width,
        raster_height,
        &hex_ownership,
        grid_width,
        grid_height,
    );
    state.entity_renderer =
        EntityRenderer::new(&state.gpu, state.terrain.camera_bind_group_layout());
    state.body_renderer = BodyRenderer::new(&state.gpu, state.terrain.camera_bind_group_layout());
}

fn rebuild_overlay(state: &mut ViewerState, hex_ownership: Vec<Option<u8>>) {
    state.hex_overlay = HexOverlayRenderer::new(
        &state.gpu,
        state.terrain.camera_bind_group_layout(),
        state.terrain.heightmap_view(),
        state.terrain.sampler(),
        state.terrain.raster_origin_x(),
        state.terrain.raster_origin_z(),
        state.terrain.raster_cell_size(),
        state.terrain.map_width(),
        state.terrain.map_height(),
        &hex_ownership,
        state.live.hex_grid_width.max(1),
        state.live.hex_grid_height.max(1),
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
                    EntityKind::Person => 0,
                    _ => 1,
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

fn project_entity_to_screen(camera: &Camera, entity: &SpectatorEntityInfo) -> Option<(f32, f32)> {
    let clip = camera.view_proj() * glam::Vec4::new(entity.x, entity.z + 2.0, entity.y, 1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if ndc.z < -1.0 || ndc.z > 1.0 {
        return None;
    }
    let screen_x = (ndc.x * 0.5 + 0.5) * camera.width;
    let screen_y = (1.0 - (ndc.y * 0.5 + 0.5)) * camera.height;
    Some((screen_x, screen_y))
}

fn pick_entity(
    camera: &Camera,
    entities: &HashMap<u32, SpectatorEntityInfo>,
    cursor_pos: Option<(f32, f32)>,
) -> Option<u32> {
    let (cursor_x, cursor_y) = cursor_pos?;
    let mut best: Option<(u32, f32)> = None;
    for entity in entities.values() {
        let Some((screen_x, screen_y)) = project_entity_to_screen(camera, entity) else {
            continue;
        };
        let dx = screen_x - cursor_x;
        let dy = screen_y - cursor_y;
        let dist_sq = dx * dx + dy * dy;
        if dist_sq > 28.0 * 28.0 {
            continue;
        }
        match best {
            Some((_, best_dist_sq)) if dist_sq >= best_dist_sq => {}
            _ => best = Some((entity.id, dist_sq)),
        }
    }
    best.map(|(id, _)| id)
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
    if let Some(needs) = update.needs {
        entity.needs = needs;
    }
    if let Some(current_goal) = update.current_goal {
        entity.current_goal = current_goal;
    }
    if let Some(current_action) = update.current_action {
        entity.current_action = current_action;
    }
    if let Some(action_queue_preview) = update.action_queue_preview {
        entity.action_queue_preview = action_queue_preview;
    }
    if let Some(decision_reason) = update.decision_reason {
        entity.decision_reason = decision_reason;
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
