use crate::camera::Camera;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::{KeyCode, PhysicalKey};

/// Tracks input state and translates events to camera actions.
pub struct InputState {
    /// Currently held keys
    keys: std::collections::HashSet<KeyCode>,
    /// Mouse button states
    middle_pressed: bool,
    right_pressed: bool,
    /// Last mouse position (for drag deltas)
    last_mouse: Option<(f64, f64)>,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            keys: std::collections::HashSet::new(),
            middle_pressed: false,
            right_pressed: false,
            last_mouse: None,
        }
    }

    pub fn key_down(&mut self, key: PhysicalKey) {
        if let PhysicalKey::Code(code) = key {
            self.keys.insert(code);
        }
    }

    pub fn key_up(&mut self, key: PhysicalKey) {
        if let PhysicalKey::Code(code) = key {
            self.keys.remove(&code);
        }
    }

    pub fn mouse_button(&mut self, button: MouseButton, state: ElementState) {
        let pressed = state == ElementState::Pressed;
        match button {
            MouseButton::Middle => self.middle_pressed = pressed,
            MouseButton::Right => self.right_pressed = pressed,
            _ => {}
        }
        if !pressed {
            // Reset last_mouse on release to avoid jump on next drag
            self.last_mouse = None;
        }
    }

    pub fn mouse_move(&mut self, x: f64, y: f64, camera: &mut Camera) {
        if let Some((lx, ly)) = self.last_mouse {
            let dx = (x - lx) as f32;
            let dy = (y - ly) as f32;

            if self.middle_pressed {
                // Orbit
                camera.orbit(-dx * 0.005, -dy * 0.005);
            } else if self.right_pressed {
                // Pan
                camera.pan(-dx, dy);
            }
        }
        self.last_mouse = Some((x, y));
    }

    pub fn mouse_wheel(&self, delta: MouseScrollDelta, camera: &mut Camera) {
        let scroll = match delta {
            MouseScrollDelta::LineDelta(_, y) => y,
            MouseScrollDelta::PixelDelta(pos) => pos.y as f32 * 0.01,
        };
        // Scroll up = zoom in (factor < 1), scroll down = zoom out (factor > 1)
        let factor = 1.0 - scroll * 0.1;
        camera.zoom(factor);
    }

    /// Apply held keyboard keys to camera each frame.
    pub fn update_camera(&self, camera: &mut Camera, dt: f32) {
        let speed = camera.distance * dt;

        if self.keys.contains(&KeyCode::KeyW) || self.keys.contains(&KeyCode::ArrowUp) {
            camera.pan(0.0, -speed);
        }
        if self.keys.contains(&KeyCode::KeyS) || self.keys.contains(&KeyCode::ArrowDown) {
            camera.pan(0.0, speed);
        }
        if self.keys.contains(&KeyCode::KeyA) || self.keys.contains(&KeyCode::ArrowLeft) {
            camera.pan(speed, 0.0);
        }
        if self.keys.contains(&KeyCode::KeyD) || self.keys.contains(&KeyCode::ArrowRight) {
            camera.pan(-speed, 0.0);
        }
        if self.keys.contains(&KeyCode::KeyQ) {
            camera.orbit(dt * 2.0, 0.0);
        }
        if self.keys.contains(&KeyCode::KeyE) {
            camera.orbit(-dt * 2.0, 0.0);
        }
    }
}
