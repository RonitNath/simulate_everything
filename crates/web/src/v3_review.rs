use serde::{Deserialize, Serialize};
use simulate_everything_engine::v3::{
    agent::AgentTrace, combat_log::CombatObservation, state::GameState,
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{info, warn};

use crate::v3_protocol::{self, SpectatorEntityInfo, V3Snapshot};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Ticks before the flagged tick to include in a review window.
pub const REVIEW_PRE_TICKS: u64 = 50;
/// Ticks after the flagged tick to include in a review window.
pub const REVIEW_POST_TICKS: u64 = 50;
/// Rolling buffer size (ticks). Buffer overwrites continuously.
pub const REVIEW_BUFFER_TICKS: usize = 200;

// ---------------------------------------------------------------------------
// Per-tick trace record
// ---------------------------------------------------------------------------

/// State recorded each tick for the rolling trace buffer.
#[derive(Debug, Clone)]
struct TickRecord {
    tick: u64,
    dt: f32,
    snapshot: V3Snapshot,
    combat_observations: Vec<CombatObservation>,
    /// Per-agent decision traces for this tick.
    agent_traces: Vec<(String, Vec<AgentTrace>)>,
}

// ---------------------------------------------------------------------------
// ReviewRecorder — rolling buffer + flag/capture
// ---------------------------------------------------------------------------

pub struct V3ReviewRecorder {
    review_dir: PathBuf,
    game_number: Option<u64>,
    seed: Option<u64>,
    agent_names: Vec<String>,
    agent_versions: Vec<String>,
    buffer: VecDeque<TickRecord>,
    pending_flags: Vec<PendingFlag>,
    active_segment: Option<ActiveSegment>,
}

struct PendingFlag {
    tick: u64,
    annotation: String,
    created_at: u64,
}

struct ActiveSegment {
    start_tick: u64,
    records: Vec<TickRecord>,
}

/// Response from flag/capture operations.
#[derive(Debug, Clone, Serialize)]
pub struct FlagResponse {
    pub id: String,
    pub tick: u64,
    pub status: String,
}

/// Summary of a review bundle for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3ReviewSummary {
    pub id: String,
    pub game_number: u64,
    pub tick: u64,
    pub annotation: String,
    pub agent_names: Vec<String>,
    pub agent_versions: Vec<String>,
    pub seed: u64,
}

impl V3ReviewRecorder {
    pub fn new(review_dir: PathBuf) -> Self {
        Self {
            review_dir,
            game_number: None,
            seed: None,
            agent_names: Vec::new(),
            agent_versions: Vec::new(),
            buffer: VecDeque::with_capacity(REVIEW_BUFFER_TICKS + 1),
            pending_flags: Vec::new(),
            active_segment: None,
        }
    }

    pub fn start_game(
        &mut self,
        game_number: u64,
        seed: u64,
        agent_names: &[String],
        agent_versions: &[String],
    ) {
        self.game_number = Some(game_number);
        self.seed = Some(seed);
        self.agent_names = agent_names.to_vec();
        self.agent_versions = agent_versions.to_vec();
        self.buffer.clear();
        self.pending_flags.clear();
        self.active_segment = None;
    }

    /// Record a tick into the rolling buffer.
    pub fn record_tick(
        &mut self,
        state: &GameState,
        dt: f32,
        combat_observations: Vec<CombatObservation>,
        agent_traces: Vec<(String, Vec<AgentTrace>)>,
    ) {
        let snapshot = v3_protocol::build_snapshot(state, dt);
        let record = TickRecord {
            tick: state.tick,
            dt,
            snapshot,
            combat_observations,
            agent_traces,
        };

        // Rolling buffer — evict oldest if full.
        if self.buffer.len() >= REVIEW_BUFFER_TICKS {
            self.buffer.pop_front();
        }
        self.buffer.push_back(record.clone());

        // Append to active segment if one exists.
        if let Some(ref mut segment) = self.active_segment {
            segment.records.push(record);
        }
    }

    /// Flag a tick for review. The window will be captured when enough
    /// post-ticks have been recorded.
    pub fn flag_tick(
        &mut self,
        game_number: u64,
        tick: u64,
        annotation: String,
    ) -> Result<FlagResponse, String> {
        let gn = self.game_number.ok_or("no game in progress")?;
        if game_number != gn {
            return Err(format!(
                "game number mismatch: expected {}, got {}",
                gn, game_number
            ));
        }

        // Check tick is within buffer range.
        let buffer_start = self.buffer.front().map(|r| r.tick).unwrap_or(0);
        let _buffer_end = self.buffer.back().map(|r| r.tick).unwrap_or(0);

        if tick < buffer_start {
            return Err(format!(
                "tick {} is before buffer start {}",
                tick, buffer_start
            ));
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.pending_flags.push(PendingFlag {
            tick,
            annotation: annotation.clone(),
            created_at: now,
        });

        let id = format!("game_{}_flag_{}", game_number, tick);
        info!("V3 review flagged: {} (annotation: {})", id, annotation);

        Ok(FlagResponse {
            id,
            tick,
            status: "pending".to_string(),
        })
    }

    /// Start a segment capture.
    pub fn start_capture(&mut self, game_number: u64) -> Result<FlagResponse, String> {
        let gn = self.game_number.ok_or("no game in progress")?;
        if game_number != gn {
            return Err("game number mismatch".to_string());
        }
        if self.active_segment.is_some() {
            return Err("segment capture already active".to_string());
        }

        let current_tick = self.buffer.back().map(|r| r.tick).unwrap_or(0);

        self.active_segment = Some(ActiveSegment {
            start_tick: current_tick,
            records: Vec::new(),
        });

        let id = format!("game_{}_segment_{}", game_number, current_tick);
        info!("V3 segment capture started: {}", id);

        Ok(FlagResponse {
            id,
            tick: current_tick,
            status: "recording".to_string(),
        })
    }

    /// Stop a segment capture and persist the bundle.
    pub async fn stop_capture(&mut self, game_number: u64) -> Result<FlagResponse, String> {
        let gn = self.game_number.ok_or("no game in progress")?;
        if game_number != gn {
            return Err("game number mismatch".to_string());
        }

        let segment = self
            .active_segment
            .take()
            .ok_or("no active segment capture")?;

        let end_tick = segment
            .records
            .last()
            .map(|r| r.tick)
            .unwrap_or(segment.start_tick);

        let id = format!("game_{}_segment_{}", game_number, segment.start_tick);

        // Write the bundle.
        let bundle_dir = self
            .review_dir
            .join(format!("game_{}", game_number))
            .join(format!("segment_{}", segment.start_tick));

        write_bundle(
            &bundle_dir,
            game_number,
            segment.start_tick,
            &format!("segment {}..{}", segment.start_tick, end_tick),
            self.seed.unwrap_or(0),
            &self.agent_names,
            &self.agent_versions,
            &segment.records,
        )
        .await
        .map_err(|e| format!("failed to write bundle: {}", e))?;

        info!(
            "V3 segment capture stopped: {} ({} ticks)",
            id,
            segment.records.len()
        );

        Ok(FlagResponse {
            id,
            tick: end_tick,
            status: "saved".to_string(),
        })
    }

    /// Collect ready flag bundles (flags where we have enough post-ticks).
    pub async fn collect_ready_flags(&mut self) {
        let buffer_end = self.buffer.back().map(|r| r.tick).unwrap_or(0);

        let game_number = match self.game_number {
            Some(gn) => gn,
            None => return,
        };

        // Check each pending flag.
        let mut ready_indices = Vec::new();
        for (i, flag) in self.pending_flags.iter().enumerate() {
            if buffer_end >= flag.tick + REVIEW_POST_TICKS {
                ready_indices.push(i);
            }
        }

        // Process ready flags (reverse order to preserve indices).
        for i in ready_indices.into_iter().rev() {
            let flag = self.pending_flags.remove(i);

            // Extract window from buffer.
            let window_start = flag.tick.saturating_sub(REVIEW_PRE_TICKS);
            let window_end = flag.tick + REVIEW_POST_TICKS;
            let records: Vec<TickRecord> = self
                .buffer
                .iter()
                .filter(|r| r.tick >= window_start && r.tick <= window_end)
                .cloned()
                .collect();

            if records.is_empty() {
                warn!(
                    "V3 review flag at tick {} has no records in window",
                    flag.tick
                );
                continue;
            }

            let bundle_dir = self
                .review_dir
                .join(format!("game_{}", game_number))
                .join(format!("flag_{}", flag.tick));

            if let Err(e) = write_bundle(
                &bundle_dir,
                game_number,
                flag.tick,
                &flag.annotation,
                self.seed.unwrap_or(0),
                &self.agent_names,
                &self.agent_versions,
                &records,
            )
            .await
            {
                warn!("Failed to write V3 review bundle: {}", e);
            } else {
                info!(
                    "V3 review bundle saved: game_{}/flag_{} ({} ticks)",
                    game_number,
                    flag.tick,
                    records.len()
                );
            }
        }
    }

    /// Finalize all pending flags on game end.
    pub async fn finalize_all(&mut self) {
        // Force-collect any remaining flags using the full buffer.
        let game_number = match self.game_number {
            Some(gn) => gn,
            None => return,
        };

        for flag in std::mem::take(&mut self.pending_flags) {
            let window_start = flag.tick.saturating_sub(REVIEW_PRE_TICKS);
            let records: Vec<TickRecord> = self
                .buffer
                .iter()
                .filter(|r| r.tick >= window_start)
                .cloned()
                .collect();

            if records.is_empty() {
                continue;
            }

            let bundle_dir = self
                .review_dir
                .join(format!("game_{}", game_number))
                .join(format!("flag_{}", flag.tick));

            if let Err(e) = write_bundle(
                &bundle_dir,
                game_number,
                flag.tick,
                &flag.annotation,
                self.seed.unwrap_or(0),
                &self.agent_names,
                &self.agent_versions,
                &records,
            )
            .await
            {
                warn!("Failed to write V3 review bundle on finalize: {}", e);
            }
        }

        // Finalize active segment if any.
        if let Some(segment) = self.active_segment.take() {
            let end_tick = segment
                .records
                .last()
                .map(|r| r.tick)
                .unwrap_or(segment.start_tick);

            let bundle_dir = self
                .review_dir
                .join(format!("game_{}", game_number))
                .join(format!("segment_{}", segment.start_tick));

            let _ = write_bundle(
                &bundle_dir,
                game_number,
                segment.start_tick,
                &format!("segment {}..{} (finalized)", segment.start_tick, end_tick),
                self.seed.unwrap_or(0),
                &self.agent_names,
                &self.agent_versions,
                &segment.records,
            )
            .await;
        }
    }

    /// Review status for the RR status endpoint.
    pub fn capturable_range(&self) -> (Option<u64>, Option<u64>) {
        let start = self.buffer.front().map(|r| r.tick);
        let end = self.buffer.back().map(|r| r.tick);
        (start, end)
    }

    pub fn has_active_capture(&self) -> bool {
        self.active_segment.is_some()
    }
}

// ---------------------------------------------------------------------------
// Bundle writing — directory of purpose-specific files
// ---------------------------------------------------------------------------

async fn write_bundle(
    dir: &Path,
    game_number: u64,
    tick: u64,
    annotation: &str,
    seed: u64,
    agent_names: &[String],
    agent_versions: &[String],
    records: &[TickRecord],
) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dir).await?;

    // summary.json
    let summary = serde_json::json!({
        "game_number": game_number,
        "tick": tick,
        "annotation": annotation,
        "seed": seed,
        "agent_names": agent_names,
        "agent_versions": agent_versions,
        "tick_range": [
            records.first().map(|r| r.tick).unwrap_or(0),
            records.last().map(|r| r.tick).unwrap_or(0),
        ],
        "tick_count": records.len(),
    });
    tokio::fs::write(
        dir.join("summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )
    .await?;

    // ascii_state.txt — entity positions, health, stacks at the flagged tick.
    let flagged_record = records
        .iter()
        .find(|r| r.tick == tick)
        .or_else(|| records.last());
    if let Some(record) = flagged_record {
        let ascii = build_ascii_state(&record.snapshot);
        tokio::fs::write(dir.join("ascii_state.txt"), ascii).await?;
    }

    // combat_log.json — all combat observations in the window.
    let combat_log: Vec<&CombatObservation> = records
        .iter()
        .flat_map(|r| r.combat_observations.iter())
        .collect();
    tokio::fs::write(
        dir.join("combat_log.json"),
        serde_json::to_string_pretty(&combat_log)?,
    )
    .await?;

    // entity_detail.json — full entity detail at the flagged tick.
    if let Some(record) = flagged_record {
        tokio::fs::write(
            dir.join("entity_detail.json"),
            serde_json::to_string_pretty(&record.snapshot.entities)?,
        )
        .await?;
    }

    // decision_trace.json — structured per-tick per-agent traces.
    let decision_trace: Vec<serde_json::Value> = records
        .iter()
        .filter(|r| !r.agent_traces.is_empty())
        .map(|r| {
            let mut agents = serde_json::Map::new();
            for (name, traces) in &r.agent_traces {
                agents.insert(
                    name.clone(),
                    serde_json::to_value(traces).unwrap_or_default(),
                );
            }
            serde_json::json!({
                "tick": r.tick,
                "agents": agents,
            })
        })
        .collect();
    tokio::fs::write(
        dir.join("decision_trace.json"),
        serde_json::to_string_pretty(&decision_trace)?,
    )
    .await?;

    Ok(())
}

/// Build an ASCII representation of the game state for Claude Code debugging.
fn build_ascii_state(snapshot: &V3Snapshot) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "=== V3 State at tick {} (dt={:.3}) ===\n\n",
        snapshot.tick, snapshot.dt
    ));

    // Players
    out.push_str("--- Players ---\n");
    for p in &snapshot.players {
        out.push_str(&format!(
            "  P{}: pop={} terr={} food={} mat={} alive={} score={}\n",
            p.id, p.population, p.territory, p.food_level, p.material_level, p.alive, p.score
        ));
    }
    out.push('\n');

    // Stacks
    if !snapshot.stacks.is_empty() {
        out.push_str("--- Stacks ---\n");
        for s in &snapshot.stacks {
            out.push_str(&format!(
                "  Stack {} (P{}): {:?} formation, {} members, center=({:.0},{:.0}), facing={:.1}rad\n",
                s.id, s.owner, s.formation, s.members.len(), s.center_x, s.center_y, s.facing
            ));
        }
        out.push('\n');
    }

    // Entities by owner
    out.push_str("--- Entities ---\n");
    let mut by_owner: std::collections::BTreeMap<Option<u8>, Vec<&SpectatorEntityInfo>> =
        std::collections::BTreeMap::new();
    for e in &snapshot.entities {
        by_owner.entry(e.owner).or_default().push(e);
    }

    for (owner, entities) in &by_owner {
        let label = match owner {
            Some(p) => format!("P{}", p),
            None => "Neutral".to_string(),
        };
        out.push_str(&format!("  {} ({} entities):\n", label, entities.len()));

        for e in entities.iter().take(20) {
            let kind = format!("{:?}", e.entity_kind);
            let role_str = e
                .role
                .as_ref()
                .map(|r| format!("{:?}", r))
                .unwrap_or_default();
            let health_str = e
                .blood
                .map(|b| format!(" blood={:.2}", b))
                .unwrap_or_default();
            let wound_str = if !e.wounds.is_empty() {
                format!(" wounds={}", e.wounds.len())
            } else {
                String::new()
            };
            let weapon_str = e
                .weapon_type
                .as_ref()
                .map(|w| format!(" wpn={}", w))
                .unwrap_or_default();
            let stack_str = e
                .stack_id
                .map(|s| format!(" stk={}", s))
                .unwrap_or_default();

            out.push_str(&format!(
                "    #{} {} {} @ ({:.0},{:.0},{:.0}) hex=({},{}){}{}{}{}{}",
                e.id,
                kind,
                role_str,
                e.x,
                e.y,
                e.z,
                e.hex_q,
                e.hex_r,
                health_str,
                wound_str,
                weapon_str,
                stack_str,
                if e.contains_count > 0 {
                    format!(" contains={}", e.contains_count)
                } else {
                    String::new()
                }
            ));
            out.push('\n');
        }
        if entities.len() > 20 {
            out.push_str(&format!("    ... and {} more\n", entities.len() - 20));
        }
    }

    // Projectiles
    if !snapshot.projectiles.is_empty() {
        out.push_str(&format!(
            "\n--- Projectiles ({}) ---\n",
            snapshot.projectiles.len()
        ));
        for p in snapshot.projectiles.iter().take(10) {
            out.push_str(&format!(
                "  #{} {:?} @ ({:.0},{:.0},{:.0}) vel=({:.0},{:.0},{:.0})\n",
                p.id, p.damage_type, p.x, p.y, p.z, p.vx, p.vy, p.vz
            ));
        }
        if snapshot.projectiles.len() > 10 {
            out.push_str(&format!(
                "  ... and {} more\n",
                snapshot.projectiles.len() - 10
            ));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Review listing
// ---------------------------------------------------------------------------

/// List all review bundle directories.
pub async fn list_reviews(review_dir: &Path) -> std::io::Result<Vec<V3ReviewSummary>> {
    let mut summaries = Vec::new();

    if !review_dir.exists() {
        return Ok(summaries);
    }

    let mut game_dirs = tokio::fs::read_dir(review_dir).await?;
    while let Some(game_entry) = game_dirs.next_entry().await? {
        if !game_entry.file_type().await?.is_dir() {
            continue;
        }

        let mut flag_dirs = tokio::fs::read_dir(game_entry.path()).await?;
        while let Some(flag_entry) = flag_dirs.next_entry().await? {
            if !flag_entry.file_type().await?.is_dir() {
                continue;
            }

            let summary_path = flag_entry.path().join("summary.json");
            if !summary_path.exists() {
                continue;
            }

            match tokio::fs::read_to_string(&summary_path).await {
                Ok(content) => {
                    if let Ok(summary) = serde_json::from_str::<V3ReviewSummary>(&content) {
                        summaries.push(summary);
                    }
                }
                Err(_) => continue,
            }
        }
    }

    Ok(summaries)
}

/// Delete a review bundle directory.
pub async fn delete_review(review_dir: &Path, id: &str) -> std::io::Result<bool> {
    // id format: game_N_flag_T or game_N_segment_T
    // Parse to find the directory.
    let parts: Vec<&str> = id.splitn(4, '_').collect();
    if parts.len() < 4 {
        return Ok(false);
    }

    let game_dir = format!("game_{}", parts[1]);
    let flag_dir = format!("{}_{}", parts[2], parts[3]);
    let path = review_dir.join(game_dir).join(flag_dir);

    if path.exists() {
        tokio::fs::remove_dir_all(&path).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}
