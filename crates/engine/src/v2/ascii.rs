use super::hex::axial_to_offset;
use super::state::GameState;

/// Renders terrain values as single digits (0-3) with hex stagger.
pub fn render_terrain(state: &GameState) -> String {
    let mut out = String::new();

    // Header: column numbers
    out.push_str("   ");
    for col in 0..state.width {
        out.push_str(&format!(" {:2}", col));
    }
    out.push('\n');

    for row in 0..state.height {
        // Odd rows are indented for hex stagger
        if row % 2 == 1 {
            out.push_str(&format!("{:2}  ", row));
        } else {
            out.push_str(&format!("{:2} ", row));
        }
        for col in 0..state.width {
            let cell = state.cell(row, col);
            let digit = cell.terrain_value.round() as u8;
            out.push_str(&format!(" {:2}", digit));
        }
        out.push('\n');
    }
    out
}

/// Renders game state with unit positions, strength, and engagement markers.
pub fn render_state(state: &GameState) -> String {
    let mut out = String::new();

    // Header: tick and player resources/unit counts
    out.push_str(&format!("Tick {}", state.tick));
    for player in &state.players {
        let label = player_label(player.id);
        let unit_count = state.units.iter().filter(|u| u.owner == player.id).count();
        out.push_str(&format!(
            " | {}: {:.1} food, {:.1} mat, {} units",
            label, player.food, player.material, unit_count
        ));
    }
    out.push('\n');

    // Column header
    out.push_str("   ");
    for col in 0..state.width {
        out.push_str(&format!(" {:4}", col));
    }
    out.push('\n');

    for row in 0..state.height {
        if row % 2 == 1 {
            out.push_str(&format!("{:2}  ", row));
        } else {
            out.push_str(&format!("{:2} ", row));
        }

        for col in 0..state.width {
            // Find the strongest unit at this cell
            let unit = state
                .units
                .iter()
                .filter(|u| {
                    let (ur, uc) = axial_to_offset(u.pos);
                    ur as usize == row && uc as usize == col
                })
                .max_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap());

            let cell_str = match unit {
                None => "....".to_string(),
                Some(u) => {
                    let base = player_label(u.owner);
                    let label: String = if u.is_general {
                        base.to_uppercase().to_string()
                    } else {
                        base.to_string()
                    };
                    let engaged = !u.engagements.is_empty();
                    let strength = u.strength.round() as i32;
                    if engaged {
                        format!("{label}*{strength:<2}")
                    } else {
                        format!("{label} {strength:<2}")
                    }
                }
            };
            out.push_str(&format!(" {:4}", cell_str));
        }
        out.push('\n');
    }
    out
}

fn player_label(id: u8) -> char {
    (b'a' + id) as char
}
