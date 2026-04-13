use std::collections::HashMap;

use super::SETTLEMENT_THRESHOLD;
use super::hex::Axial;
use super::state::{GameState, Role};

// ---------------------------------------------------------------------------
// Event types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum GameEvent {
    UnitProduced {
        tick: u64,
        player: u8,
        unit_id: u32,
        pos: Axial,
    },
    UnitKilled {
        tick: u64,
        player: u8,
        unit_id: u32,
        pos: Axial,
        killer: Option<u8>,
    },
    EngagementCreated {
        tick: u64,
        attacker: u32,
        target: u32,
        attacker_owner: u8,
        target_owner: u8,
    },
    SettlementFounded {
        tick: u64,
        player: u8,
        pos: Axial,
    },
    PlayerEliminated {
        tick: u64,
        player: u8,
    },
}

impl GameEvent {
    fn tick(&self) -> u64 {
        match self {
            GameEvent::UnitProduced { tick, .. }
            | GameEvent::UnitKilled { tick, .. }
            | GameEvent::EngagementCreated { tick, .. }
            | GameEvent::SettlementFounded { tick, .. }
            | GameEvent::PlayerEliminated { tick, .. } => *tick,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnitPositionSample {
    pub tick: u64,
    pub player: u8,
    pub unit_id: u32,
    pub q: i32,
    pub r: i32,
    pub strength: f32,
    pub engaged: bool,
}

#[derive(Debug, Clone)]
pub struct EconomySample {
    pub tick: u64,
    pub player: u8,
    pub food_stockpile: f32,
    pub material_stockpile: f32,
    pub population: u16,
    pub farmers: u16,
    pub workers: u16,
    pub soldiers: u16,
    pub units: usize,
    pub territory: usize,
    pub settlements: usize,
}

#[derive(Debug, Clone)]
pub struct AgentPollRecord {
    pub tick: u64,
    pub player: u8,
    pub move_count: u16,
    pub engage_count: u16,
    pub produce_count: u16,
    pub other_count: u16,
    pub mode: Option<String>,
}

// ---------------------------------------------------------------------------
// GameLog
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GameLog {
    pub events: Vec<GameEvent>,
    pub economy_samples: Vec<EconomySample>,
    pub agent_polls: Vec<AgentPollRecord>,
    pub unit_positions: Vec<UnitPositionSample>,
    known_settlements: Vec<(u8, Axial)>,
}

impl GameLog {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            economy_samples: Vec::new(),
            agent_polls: Vec::new(),
            unit_positions: Vec::new(),
            known_settlements: Vec::new(),
        }
    }

    pub fn record(&mut self, event: GameEvent) {
        self.events.push(event);
    }

    pub fn record_economy(&mut self, sample: EconomySample) {
        self.economy_samples.push(sample);
    }

    pub fn record_poll(&mut self, poll: AgentPollRecord) {
        self.agent_polls.push(poll);
    }

    /// Build an economy sample for one player from current game state.
    pub fn sample_economy(state: &GameState, player_id: u8) -> EconomySample {
        let units = state
            .units
            .values()
            .filter(|u| u.owner == player_id)
            .count();
        let territory = state
            .grid
            .iter()
            .filter(|c| c.stockpile_owner == Some(player_id))
            .count();

        let mut population: u16 = 0;
        let mut farmers: u16 = 0;
        let mut workers: u16 = 0;
        let mut soldiers: u16 = 0;
        for pop in state.population.values().filter(|p| p.owner == player_id) {
            population += pop.count;
            match pop.role {
                Role::Farmer => farmers += pop.count,
                Role::Worker => workers += pop.count,
                Role::Soldier => soldiers += pop.count,
                Role::Idle => {}
            }
        }

        let settlements = count_settlements(state, player_id);

        let player = state.players.iter().find(|p| p.id == player_id);
        let food_stockpile = player.map(|p| p.food).unwrap_or(0.0);
        let material_stockpile = player.map(|p| p.material).unwrap_or(0.0);

        EconomySample {
            tick: state.tick,
            player: player_id,
            food_stockpile,
            material_stockpile,
            population,
            farmers,
            workers,
            soldiers,
            units,
            territory,
            settlements,
        }
    }

    /// Detect newly founded settlements by comparing against known list.
    /// Must be called with pre-collected settlement data to avoid borrow conflicts.
    pub fn detect_new_settlements(&mut self, tick: u64, current_settlements: &[(u8, Axial)]) {
        for &(player, pos) in current_settlements {
            if !self.known_settlements.contains(&(player, pos)) {
                self.known_settlements.push((player, pos));
                self.record(GameEvent::SettlementFounded { tick, player, pos });
            }
        }
    }
}

fn count_settlements(state: &GameState, player_id: u8) -> usize {
    let mut seen: Vec<Axial> = Vec::new();
    for pop in state.population.values().filter(|p| p.owner == player_id) {
        if !seen.contains(&pop.hex)
            && state.population_on_hex(player_id, pop.hex) >= SETTLEMENT_THRESHOLD
        {
            seen.push(pop.hex);
        }
    }
    seen.len()
}

/// Collect all current settlements as (player, pos) pairs.
pub fn collect_settlements(state: &GameState) -> Vec<(u8, Axial)> {
    let mut result = Vec::new();
    for player in &state.players {
        if !player.alive {
            continue;
        }
        let mut seen: Vec<Axial> = Vec::new();
        for pop in state.population.values().filter(|p| p.owner == player.id) {
            if !seen.contains(&pop.hex)
                && state.population_on_hex(player.id, pop.hex) >= SETTLEMENT_THRESHOLD
            {
                seen.push(pop.hex);
                result.push((player.id, pop.hex));
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Postmortem summary
// ---------------------------------------------------------------------------

pub struct PlayerStats {
    pub units_produced: u32,
    pub units_lost: u32,
    pub kills: u32,
    pub peak_units: usize,
    pub peak_territory: usize,
    pub final_population: u16,
    pub final_territory: usize,
    pub settlements_founded: u32,
}

pub struct PostmortemSummary {
    pub winner: Option<u8>,
    pub agent_names: Vec<String>,
    pub final_tick: u64,
    pub timed_out: bool,
    pub timeline: Vec<String>,
    pub player_stats: Vec<PlayerStats>,
    pub decisive_tick: Option<u64>,
    pub decisive_reason: Option<String>,
}

impl GameLog {
    pub fn summarize(
        &self,
        agent_names: &[String],
        winner: Option<u8>,
        final_tick: u64,
        timed_out: bool,
    ) -> PostmortemSummary {
        let num_players = agent_names.len();
        let mut timeline = Vec::new();
        let mut player_stats: Vec<PlayerStats> = (0..num_players)
            .map(|_| PlayerStats {
                units_produced: 0,
                units_lost: 0,
                kills: 0,
                peak_units: 0,
                peak_territory: 0,
                final_population: 0,
                final_territory: 0,
                settlements_founded: 0,
            })
            .collect();

        // Track first events per player
        let mut first_produced: Vec<bool> = vec![false; num_players];
        let mut first_contact_recorded = false;

        // Track mode transitions
        let mut last_mode: Vec<Option<String>> = vec![None; num_players];

        // Aggregate kills into battles (clusters within 20 ticks)
        let mut kill_clusters: Vec<(u64, u64, HashMap<u8, u32>, HashMap<u8, u32>, Axial)> =
            Vec::new();

        for event in &self.events {
            match event {
                GameEvent::UnitProduced { tick, player, .. } => {
                    let pid = *player as usize;
                    if pid < num_players {
                        player_stats[pid].units_produced += 1;
                        if !first_produced[pid] {
                            first_produced[pid] = true;
                            timeline
                                .push(format!("  t={:<6} P{}: first unit produced", tick, player));
                        }
                    }
                }
                GameEvent::UnitKilled {
                    tick,
                    player,
                    pos,
                    killer,
                    ..
                } => {
                    let pid = *player as usize;
                    if pid < num_players {
                        player_stats[pid].units_lost += 1;
                    }
                    if let Some(k) = killer {
                        let kid = *k as usize;
                        if kid < num_players {
                            player_stats[kid].kills += 1;
                        }
                    }
                    // Cluster kills into battles
                    if let Some(last) = kill_clusters.last_mut() {
                        if *tick <= last.1 + 20 {
                            last.1 = *tick;
                            *last.2.entry(*player).or_insert(0) += 1;
                            if let Some(k) = killer {
                                *last.3.entry(*k).or_insert(0) += 1;
                            }
                        } else {
                            kill_clusters.push((
                                *tick,
                                *tick,
                                HashMap::from([(*player, 1)]),
                                killer.map(|k| HashMap::from([(k, 1)])).unwrap_or_default(),
                                *pos,
                            ));
                        }
                    } else {
                        kill_clusters.push((
                            *tick,
                            *tick,
                            HashMap::from([(*player, 1)]),
                            killer.map(|k| HashMap::from([(k, 1)])).unwrap_or_default(),
                            *pos,
                        ));
                    }
                }
                GameEvent::EngagementCreated {
                    tick,
                    attacker_owner,
                    target_owner,
                    ..
                } => {
                    if !first_contact_recorded && attacker_owner != target_owner {
                        first_contact_recorded = true;
                        timeline.push(format!(
                            "  t={:<6} First contact: P{} vs P{}",
                            tick, attacker_owner, target_owner
                        ));
                    }
                }
                GameEvent::SettlementFounded { tick, player, pos } => {
                    let pid = *player as usize;
                    if pid < num_players {
                        player_stats[pid].settlements_founded += 1;
                    }
                    timeline.push(format!(
                        "  t={:<6} P{}: settlement at ({},{})",
                        tick, player, pos.q, pos.r
                    ));
                }
                GameEvent::PlayerEliminated { tick, player } => {
                    timeline.push(format!("  t={:<6} P{} eliminated", tick, player));
                }
            }
        }

        // Add battle summaries to timeline
        for (start, end, deaths, _killers, pos) in &kill_clusters {
            let total_deaths: u32 = deaths.values().sum();
            if total_deaths >= 2 {
                let mut parts: Vec<String> = deaths
                    .iter()
                    .map(|(p, n)| format!("P{} -{}", p, n))
                    .collect();
                parts.sort();
                let tick_label = if start == end {
                    format!("{}", start)
                } else {
                    format!("{}-{}", start, end)
                };
                timeline.push(format!(
                    "  t={:<6} Battle near ({},{}): {}",
                    tick_label,
                    pos.q,
                    pos.r,
                    parts.join(", ")
                ));
            }
        }

        // Add mode transitions from agent polls
        for poll in &self.agent_polls {
            let pid = poll.player as usize;
            if pid >= num_players {
                continue;
            }
            if poll.mode != last_mode[pid] {
                if let Some(ref mode) = poll.mode {
                    timeline.push(format!(
                        "  t={:<6} P{} {} → {} mode",
                        poll.tick, poll.player, agent_names[pid], mode
                    ));
                }
                last_mode[pid] = poll.mode.clone();
            }
        }

        // Sort timeline by tick (extract tick from the "  t=N" prefix)
        timeline.sort_by_key(|line| {
            line.trim()
                .strip_prefix("t=")
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| s.split('-').next())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        });

        // Cap at 25 entries
        if timeline.len() > 25 {
            let keep = 25;
            // Always keep first 2 and last 3
            let head: Vec<_> = timeline[..2].to_vec();
            let tail: Vec<_> = timeline[timeline.len() - 3..].to_vec();
            let middle_budget = keep - head.len() - tail.len();
            let middle: Vec<_> = timeline[2..timeline.len() - 3]
                .iter()
                .step_by((timeline.len() - 5).max(1) / middle_budget.max(1) + 1)
                .cloned()
                .take(middle_budget)
                .collect();
            timeline = head;
            timeline.extend(middle);
            timeline.extend(tail);
        }

        // Peak stats from economy samples
        for sample in &self.economy_samples {
            let pid = sample.player as usize;
            if pid < num_players {
                if sample.units > player_stats[pid].peak_units {
                    player_stats[pid].peak_units = sample.units;
                }
                if sample.territory > player_stats[pid].peak_territory {
                    player_stats[pid].peak_territory = sample.territory;
                }
            }
        }

        // Final stats from last economy sample per player
        for pid in 0..num_players {
            if let Some(last) = self
                .economy_samples
                .iter()
                .rev()
                .find(|s| s.player == pid as u8)
            {
                player_stats[pid].final_population = last.population;
                player_stats[pid].final_territory = last.territory;
            }
        }

        // Game end
        let end_reason = if timed_out {
            "timeout"
        } else {
            "last settlement lost"
        };
        timeline.push(format!("  t={:<6} Game ends ({})", final_tick, end_reason));

        // Decisive moment detection
        let (decisive_tick, decisive_reason) =
            detect_decisive_moment(&self.economy_samples, &kill_clusters, winner, num_players);

        PostmortemSummary {
            winner,
            agent_names: agent_names.to_vec(),
            final_tick,
            timed_out,
            timeline,
            player_stats,
            decisive_tick,
            decisive_reason,
        }
    }
}

fn detect_decisive_moment(
    samples: &[EconomySample],
    kill_clusters: &[(u64, u64, HashMap<u8, u32>, HashMap<u8, u32>, Axial)],
    winner: Option<u8>,
    num_players: usize,
) -> (Option<u64>, Option<String>) {
    let Some(winner_id) = winner else {
        return (None, Some("draw — no decisive moment".to_string()));
    };

    // Walk economy samples: find tick where winner first had 2:1 unit advantage
    // that was never reversed.
    let ticks: Vec<u64> = samples
        .iter()
        .map(|s| s.tick)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let mut decisive_tick: Option<u64> = None;
    for &tick in &ticks {
        let winner_units = samples
            .iter()
            .find(|s| s.tick == tick && s.player == winner_id)
            .map(|s| s.units)
            .unwrap_or(0);

        let max_other = (0..num_players as u8)
            .filter(|&p| p != winner_id)
            .filter_map(|p| samples.iter().find(|s| s.tick == tick && s.player == p))
            .map(|s| s.units)
            .max()
            .unwrap_or(0);

        if max_other > 0 && winner_units >= max_other * 2 {
            // Check if this advantage was ever reversed
            let reversed = ticks.iter().filter(|&&t| t > tick).any(|&t| {
                let wu = samples
                    .iter()
                    .find(|s| s.tick == t && s.player == winner_id)
                    .map(|s| s.units)
                    .unwrap_or(0);
                let mo = (0..num_players as u8)
                    .filter(|&p| p != winner_id)
                    .filter_map(|p| samples.iter().find(|s| s.tick == t && s.player == p))
                    .map(|s| s.units)
                    .max()
                    .unwrap_or(0);
                mo > 0 && wu < mo * 3 / 2 // reversed if advantage drops below 1.5:1
            });
            if !reversed {
                decisive_tick = Some(tick);
                break;
            }
        }
    }

    if let Some(dt) = decisive_tick {
        let winner_units = samples
            .iter()
            .find(|s| s.tick == dt && s.player == winner_id)
            .map(|s| s.units)
            .unwrap_or(0);
        let reason = format!(
            "P{} achieved 2:1 unit advantage ({} units) — never reversed",
            winner_id, winner_units
        );
        return (Some(dt), Some(reason));
    }

    // Fallback: find worst battle for the loser
    for (start, _end, deaths, _killers, pos) in kill_clusters {
        let loser_deaths: u32 = deaths
            .iter()
            .filter(|(p, _)| **p != winner_id)
            .map(|(_, &n)| n)
            .sum();
        let winner_deaths = deaths.get(&winner_id).copied().unwrap_or(0);
        if loser_deaths > winner_deaths && loser_deaths >= 2 {
            let reason = format!(
                "battle near ({},{}) — loser lost {} units vs {} for winner",
                pos.q, pos.r, loser_deaths, winner_deaths
            );
            return (Some(*start), Some(reason));
        }
    }

    (None, Some("gradual attrition".to_string()))
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

impl PostmortemSummary {
    pub fn render(&self) -> String {
        let mut out = String::new();

        let matchup = self.agent_names.join(" vs ");
        out.push_str(&format!("=== POSTMORTEM: {} ===\n", matchup));

        let winner_str = match self.winner {
            Some(w) => {
                let name = self
                    .agent_names
                    .get(w as usize)
                    .map(|s| s.as_str())
                    .unwrap_or("?");
                let reason = if self.timed_out {
                    "timeout"
                } else {
                    "last settlement lost"
                };
                format!("P{} ({}) at tick {} ({})", w, name, self.final_tick, reason)
            }
            None => format!("draw at tick {}", self.final_tick),
        };
        out.push_str(&format!("Winner: {}\n\n", winner_str));

        out.push_str("--- TIMELINE ---\n");
        for line in &self.timeline {
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');

        // Stats table
        out.push_str("--- STATS ---\n");
        let n = self.player_stats.len();
        let col_width = 16;

        // Header
        out.push_str(&format!("{:<12}", ""));
        for i in 0..n {
            let label = format!(
                "P{} ({})",
                i,
                self.agent_names.get(i).map(|s| s.as_str()).unwrap_or("?")
            );
            out.push_str(&format!("{:<width$}", label, width = col_width));
        }
        out.push('\n');

        // Rows
        let rows: Vec<(&str, Box<dyn Fn(&PlayerStats) -> String>)> = vec![
            (
                "Produced",
                Box::new(|s: &PlayerStats| format!("{}", s.units_produced)),
            ),
            (
                "Lost",
                Box::new(|s: &PlayerStats| format!("{}", s.units_lost)),
            ),
            (
                "K/D",
                Box::new(|s: &PlayerStats| {
                    if s.units_lost == 0 {
                        format!("{:.0}/0", s.kills)
                    } else {
                        format!("{:.2}", s.kills as f64 / s.units_lost as f64)
                    }
                }),
            ),
            (
                "Peak units",
                Box::new(|s: &PlayerStats| format!("{}", s.peak_units)),
            ),
            (
                "Territory",
                Box::new(|s: &PlayerStats| format!("{}", s.final_territory)),
            ),
            (
                "Population",
                Box::new(|s: &PlayerStats| format!("{}", s.final_population)),
            ),
            (
                "Settlements",
                Box::new(|s: &PlayerStats| format!("{}", s.settlements_founded)),
            ),
        ];

        for (label, fmt_fn) in &rows {
            out.push_str(&format!("{:<12}", label));
            for stats in &self.player_stats {
                out.push_str(&format!("{:<width$}", fmt_fn(stats), width = col_width));
            }
            out.push('\n');
        }
        out.push('\n');

        // Decisive moment
        out.push_str("--- DECISIVE MOMENT ---\n");
        match (&self.decisive_tick, &self.decisive_reason) {
            (Some(tick), Some(reason)) => {
                out.push_str(&format!("t={}: {}\n", tick, reason));
            }
            (None, Some(reason)) => {
                out.push_str(&format!("{}\n", reason));
            }
            _ => {
                out.push_str("no clear turning point\n");
            }
        }

        out
    }

    pub fn one_liner(&self) -> String {
        let winner_str = match self.winner {
            Some(w) => {
                let name = self
                    .agent_names
                    .get(w as usize)
                    .map(|s| s.as_str())
                    .unwrap_or("?");
                format!("P{}({}) wins t={}", w, name, self.final_tick)
            }
            None => format!("draw t={}", self.final_tick),
        };
        let reason = self.decisive_reason.as_deref().unwrap_or("no clear cause");
        format!("{} — {}", winner_str, reason)
    }
}

// ---------------------------------------------------------------------------
// Loss categorization
// ---------------------------------------------------------------------------

pub fn categorize_loss(summary: &PostmortemSummary, loser: u8) -> &'static str {
    let lid = loser as usize;
    if lid >= summary.player_stats.len() {
        return "unknown";
    }
    let stats = &summary.player_stats[lid];
    let winner = match summary.winner {
        Some(w) => w as usize,
        None => return "draw",
    };
    if winner >= summary.player_stats.len() {
        return "unknown";
    }
    let winner_stats = &summary.player_stats[winner];

    // General killed early with few units
    if stats.units_lost > 0 && stats.kills as f64 / stats.units_lost as f64 <= 0.5 {
        return "combat disadvantage";
    }
    if stats.settlements_founded + 1 < winner_stats.settlements_founded {
        return "economy gap";
    }
    if stats.peak_units > 0 && stats.peak_units <= winner_stats.peak_units / 2 {
        return "outproduced";
    }
    if stats.units_produced < winner_stats.units_produced / 2 {
        return "slow start";
    }
    "attrition"
}
