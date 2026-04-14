use crate::camera::Camera;
use glam::Vec3;
use simulate_everything_protocol::{EntityKind, Role, SpectatorEntityInfo, StructureType};
use std::collections::HashMap;

pub struct OverlayUi {
    root: web_sys::Element,
}

struct OverlayEntry {
    entity_id: u32,
    screen_x: f32,
    screen_y: f32,
    distance: f32,
    title: String,
    subtitle: Option<String>,
    owner: u8,
    blood: Option<f32>,
    stamina: Option<f32>,
}

impl OverlayUi {
    pub fn new() -> Self {
        let document = web_sys::window().unwrap().document().unwrap();
        let overlay_host = document
            .get_element_by_id("solid-ui")
            .expect("viewer overlay host missing");
        let root = document.create_element("div").unwrap();
        root.set_class_name("viewer-overlay-root");
        overlay_host.append_child(&root).unwrap();
        Self { root }
    }

    pub fn update(
        &self,
        camera: &Camera,
        entities: &HashMap<u32, SpectatorEntityInfo>,
        selected_entity_id: Option<u32>,
    ) {
        let mut entries = entities
            .values()
            .filter_map(|entity| overlay_entry(camera, entity))
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        entries.truncate(40);

        let mut html = String::new();
        let selected = selected_entity_id.and_then(|id| {
            entities
                .get(&id)
                .and_then(|entity| overlay_entry(camera, entity))
        });

        if let Some(selected) = selected {
            html.push_str(&format!(
                "<div class=\"viewer-selection-ring\" style=\"left:{:.1}px;top:{:.1}px;border-color:{}\"></div>",
                selected.screen_x,
                selected.screen_y,
                owner_color(selected.owner),
            ));
            if !entries.iter().any(|entry| entry.entity_id == selected.entity_id) {
                entries.insert(0, selected);
            }
        }

        for entry in entries {
            let left = entry.screen_x.round();
            let top = entry.screen_y.round();
            html.push_str(&format!(
                "<div class=\"viewer-entity-overlay\" style=\"left:{left}px;top:{top}px\">"
            ));
            html.push_str(&format!(
                "<div class=\"viewer-label\" style=\"border-color:{}\">{}</div>",
                owner_color(entry.owner),
                escape_html(&entry.title)
            ));
            if let Some(subtitle) = entry.subtitle {
                html.push_str(&format!(
                    "<div class=\"viewer-subtitle\">{}</div>",
                    escape_html(&subtitle)
                ));
            }
            if let Some(blood) = entry.blood.filter(|value| *value < 0.995) {
                html.push_str(&bar_html("viewer-bar viewer-bar-blood", blood));
            }
            if let Some(stamina) = entry.stamina.filter(|value| *value < 0.995) {
                html.push_str(&bar_html("viewer-bar viewer-bar-stamina", stamina));
            }
            html.push_str("</div>");
        }

        self.root.set_inner_html(&html);
    }
}

fn overlay_entry(camera: &Camera, entity: &SpectatorEntityInfo) -> Option<OverlayEntry> {
    let pos = Vec3::from_array([entity.x, entity.z + 3.5, entity.y]);
    let screen = project_to_screen(camera, pos)?;
    let entity_pos = Vec3::from_array([entity.x, entity.z, entity.y]);
    let distance = camera.eye().distance(entity_pos);
    if distance > 450.0 && entity.id != 0 {
        return None;
    }

    Some(OverlayEntry {
        entity_id: entity.id,
        screen_x: screen[0],
        screen_y: screen[1],
        distance,
        title: entity_title(entity),
        subtitle: entity_subtitle(entity),
        owner: entity.owner.unwrap_or(0),
        blood: entity.blood,
        stamina: entity.stamina,
    })
}

fn project_to_screen(camera: &Camera, world_pos: Vec3) -> Option<[f32; 2]> {
    let clip = camera.view_proj() * world_pos.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if ndc.z < -1.0 || ndc.z > 1.0 {
        return None;
    }
    let screen_x = (ndc.x * 0.5 + 0.5) * camera.width;
    let screen_y = (1.0 - (ndc.y * 0.5 + 0.5)) * camera.height;
    if screen_x < -120.0
        || screen_x > camera.width + 120.0
        || screen_y < -120.0
        || screen_y > camera.height + 120.0
    {
        return None;
    }
    Some([screen_x, screen_y])
}

fn entity_title(entity: &SpectatorEntityInfo) -> String {
    let owner = entity.owner.unwrap_or(0);
    match entity.entity_kind {
        EntityKind::Person => {
            let role = entity.role.map(role_label).unwrap_or("Person");
            format!("P{owner} {role}")
        }
        EntityKind::Structure => {
            let structure = entity.structure_type.map(structure_label).unwrap_or("Structure");
            format!("P{owner} {structure}")
        }
    }
}

fn entity_subtitle(entity: &SpectatorEntityInfo) -> Option<String> {
    if let Some(task) = entity.current_task.as_ref().filter(|task| !task.is_empty()) {
        return Some(task.clone());
    }
    if let Some(weapon) = entity.weapon_type.as_ref().filter(|weapon| !weapon.is_empty()) {
        return Some(weapon.clone());
    }
    None
}

fn role_label(role: Role) -> &'static str {
    match role {
        Role::Idle => "Idle",
        Role::Farmer => "Farmer",
        Role::Worker => "Worker",
        Role::Soldier => "Soldier",
        Role::Builder => "Builder",
    }
}

fn structure_label(structure: StructureType) -> &'static str {
    match structure {
        StructureType::Farm => "Farm",
        StructureType::Village => "Village",
        StructureType::City => "City",
        StructureType::Depot => "Depot",
        StructureType::Wall => "Wall",
        StructureType::Tower => "Tower",
        StructureType::Workshop => "Workshop",
    }
}

fn owner_color(owner: u8) -> &'static str {
    match owner % 8 {
        0 => "#4b76ff",
        1 => "#ff5c63",
        2 => "#37c871",
        3 => "#f2c94c",
        4 => "#bb6bd9",
        5 => "#3bc9db",
        6 => "#ff9f43",
        _ => "#c7c7c7",
    }
}

fn bar_html(class_name: &str, frac: f32) -> String {
    let width = (frac.clamp(0.0, 1.0) * 100.0).round();
    format!(
        "<div class=\"{class_name}\"><div class=\"viewer-bar-fill\" style=\"width:{width}%\"></div></div>"
    )
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
