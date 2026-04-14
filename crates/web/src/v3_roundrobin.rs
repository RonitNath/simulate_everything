use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::{Mutex, Notify, broadcast};
use tracing::info;

use simulate_everything_engine::v3::{
    agent::{AgentTrace, LayeredAgent},
    damage_table::DamageEstimateTable,
    mapgen,
    operations::SharedOperationsLayer,
    sim,
    state::GameState,
    strategy::{SpreadStrategy, StrikerStrategy, TurtleStrategy},
    tactical::SharedTacticalLayer,
};

use crate::v3_protocol::{self, TimeMode, V3RrStatus, V3ServerToSpectator};
use crate::v3_review::{self, FlagResponse, V3ReviewRecorder, V3ReviewSummary};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum ticks per game before timeout.
const MAX_TICKS: u64 = 5000;
/// Map width in hexes for RR games.
const MAP_WIDTH: usize = 30;
/// Map height in hexes for RR games.
const MAP_HEIGHT: usize = 30;
/// Number of players per RR game.
const NUM_PLAYERS: u8 = 2;
/// Broadcast channel capacity.
const BROADCAST_CAPACITY: usize = 512;
/// Delay between games when autoplay is on (ms).
const INTER_GAME_DELAY_MS: u64 = 500;

// ---------------------------------------------------------------------------
// Snapshot cache for late-joining spectators
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct V3RrSnapshot {
    init: Option<V3ServerToSpectator>,
    latest_snapshot: Option<V3ServerToSpectator>,
}

// ---------------------------------------------------------------------------
// V3RoundRobin
// ---------------------------------------------------------------------------

pub struct V3RoundRobin {
    tick_ms: AtomicU64,
    mode: Mutex<TimeMode>,
    autoplay: AtomicBool,
    spectator_tx: broadcast::Sender<V3ServerToSpectator>,
    snapshot: Arc<Mutex<V3RrSnapshot>>,
    review: Arc<Mutex<V3ReviewRecorder>>,
    review_dir: PathBuf,
    paused: AtomicBool,
    resume_notify: Notify,
    reset_flag: AtomicBool,
    game_number: AtomicU64,
    current_tick: AtomicU64,
}

impl V3RoundRobin {
    pub fn new(tick_ms: u64, review_dir: PathBuf) -> Self {
        let (spectator_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            tick_ms: AtomicU64::new(tick_ms),
            mode: Mutex::new(TimeMode::Tactical),
            autoplay: AtomicBool::new(true),
            spectator_tx,
            snapshot: Arc::new(Mutex::new(V3RrSnapshot::default())),
            review: Arc::new(Mutex::new(V3ReviewRecorder::new(review_dir.clone()))),
            review_dir,
            paused: AtomicBool::new(false),
            resume_notify: Notify::new(),
            reset_flag: AtomicBool::new(false),
            game_number: AtomicU64::new(0),
            current_tick: AtomicU64::new(0),
        }
    }

    // -- Control API --

    pub fn set_tick_ms(&self, ms: u64) {
        let clamped = ms.clamp(10, 5000);
        self.tick_ms.store(clamped, Ordering::Relaxed);
        info!("V3 RR tick speed set to {}ms", clamped);
    }

    pub fn get_tick_ms(&self) -> u64 {
        self.tick_ms.load(Ordering::Relaxed)
    }

    pub async fn set_mode(&self, mode: TimeMode) {
        *self.mode.lock().await = mode;
        info!("V3 RR mode set to {:?}", mode);
    }

    pub async fn get_mode(&self) -> TimeMode {
        *self.mode.lock().await
    }

    pub fn set_autoplay(&self, on: bool) {
        self.autoplay.store(on, Ordering::Relaxed);
        info!("V3 RR autoplay set to {}", on);
    }

    pub fn get_autoplay(&self) -> bool {
        self.autoplay.load(Ordering::Relaxed)
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
        info!("V3 RR paused");
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
        self.resume_notify.notify_one();
        info!("V3 RR resumed");
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn reset(&self) {
        self.reset_flag.store(true, Ordering::Relaxed);
        self.paused.store(false, Ordering::Relaxed);
        self.resume_notify.notify_one();
        info!("V3 RR reset requested");
    }

    // -- Spectator management --

    pub fn spectator_subscribe(&self) -> broadcast::Receiver<V3ServerToSpectator> {
        self.spectator_tx.subscribe()
    }

    pub async fn spectator_catchup(&self) -> Vec<V3ServerToSpectator> {
        let snap = self.snapshot.lock().await;
        let mut msgs = Vec::new();
        if let Some(ref init) = snap.init {
            msgs.push(init.clone());
        }
        if let Some(ref snapshot) = snap.latest_snapshot {
            msgs.push(snapshot.clone());
        }
        drop(snap);
        // Include current RR status.
        msgs.push(V3ServerToSpectator::RrStatus(self.build_rr_status().await));
        msgs
    }

    async fn broadcast(&self, msg: V3ServerToSpectator) {
        let mut snap = self.snapshot.lock().await;
        match &msg {
            V3ServerToSpectator::Init { .. } => {
                snap.init = Some(msg.clone());
                snap.latest_snapshot = None;
            }
            V3ServerToSpectator::Snapshot { .. } => {
                snap.latest_snapshot = Some(msg.clone());
            }
            V3ServerToSpectator::GameEnd { .. } => {
                snap.init = None;
                snap.latest_snapshot = None;
            }
            V3ServerToSpectator::SnapshotDelta { .. }
            | V3ServerToSpectator::Config { .. }
            | V3ServerToSpectator::RrStatus(_) => {}
        }
        drop(snap);
        let _ = self.spectator_tx.send(msg);
    }

    pub async fn broadcast_rr_status(&self) {
        let status = self.build_rr_status().await;
        let _ = self
            .spectator_tx
            .send(V3ServerToSpectator::RrStatus(status));
    }

    pub async fn broadcast_config(
        &self,
        tick_ms: Option<u64>,
        mode: Option<TimeMode>,
        autoplay: Option<bool>,
    ) {
        self.broadcast(V3ServerToSpectator::Config {
            tick_ms,
            mode,
            autoplay,
        })
        .await;
    }

    async fn build_rr_status(&self) -> V3RrStatus {
        let mode = *self.mode.lock().await;
        let review = self.review.lock().await;
        let (cap_start, cap_end) = review.capturable_range();
        let active = if review.has_active_capture() {
            Some("recording".to_string())
        } else {
            None
        };
        drop(review);
        V3RrStatus {
            game_number: self.game_number.load(Ordering::Relaxed),
            current_tick: self.current_tick.load(Ordering::Relaxed),
            dt: mode.dt(),
            mode,
            paused: self.is_paused(),
            tick_ms: self.get_tick_ms(),
            autoplay: self.get_autoplay(),
            capturable_start_tick: cap_start,
            capturable_end_tick: cap_end,
            active_capture: active,
        }
    }

    // -- Review API --

    pub async fn flag_tick(
        &self,
        game_number: u64,
        tick: u64,
        annotation: String,
    ) -> Result<FlagResponse, String> {
        self.review
            .lock()
            .await
            .flag_tick(game_number, tick, annotation)
    }

    pub async fn start_capture(&self, game_number: u64) -> Result<FlagResponse, String> {
        self.review.lock().await.start_capture(game_number)
    }

    pub async fn stop_capture(&self, game_number: u64) -> Result<FlagResponse, String> {
        self.review.lock().await.stop_capture(game_number).await
    }

    pub async fn list_reviews(&self) -> std::io::Result<Vec<V3ReviewSummary>> {
        v3_review::list_reviews(&self.review_dir).await
    }

    pub async fn delete_review(&self, id: &str) -> std::io::Result<bool> {
        v3_review::delete_review(&self.review_dir, id).await
    }

    // -- Internal helpers --

    async fn wait_if_paused(&self) {
        while self.paused.load(Ordering::Relaxed) {
            self.resume_notify.notified().await;
        }
    }

    fn take_reset(&self) -> bool {
        self.reset_flag.swap(false, Ordering::Relaxed)
    }

    // -- Main loop --

    pub async fn run_loop(self: Arc<Self>) {
        let mut seed: u64 = 1000;

        loop {
            if self.take_reset() {
                info!("V3 RR reset");
            }

            // Wait if paused, or if autoplay is off and we're between games.
            self.wait_if_paused().await;

            let game_number = seed - 999;
            self.game_number.store(game_number, Ordering::Relaxed);
            self.current_tick.store(0, Ordering::Relaxed);

            // Generate map.
            let mut state = mapgen::generate(MAP_WIDTH, MAP_HEIGHT, NUM_PLAYERS, seed);

            // Create agents — alternate between personality types.
            let (mut agents, agent_names, agent_versions) = create_agents(NUM_PLAYERS, game_number);

            info!(
                "V3 RR game #{}: {} (seed={})",
                game_number,
                agent_names.join(", "),
                seed
            );

            // Start review recording for this game.
            {
                let mut review = self.review.lock().await;
                review.start_game(game_number, seed, &agent_names, &agent_versions);
            }

            // Broadcast init.
            let init = v3_protocol::build_init(&state, &agent_names, &agent_versions, game_number);
            self.broadcast(V3ServerToSpectator::Init { init, game_number })
                .await;

            // Broadcast initial full snapshot + seed delta tracker.
            let mode = *self.mode.lock().await;
            let dt = mode.dt();
            let full_snapshot = v3_protocol::build_snapshot(&state, dt);
            let mut delta_tracker = v3_protocol::DeltaTracker::new();
            delta_tracker.seed_from_snapshot(&full_snapshot);
            self.broadcast(V3ServerToSpectator::Snapshot {
                snapshot: full_snapshot,
            })
            .await;

            self.broadcast_rr_status().await;

            let mut aborted = false;
            let mut game_over = false;

            while state.tick < MAX_TICKS && !game_over {
                if self.reset_flag.load(Ordering::Relaxed) {
                    aborted = true;
                    break;
                }

                self.wait_if_paused().await;

                let tick_start = tokio::time::Instant::now();
                let tick_ms = self.get_tick_ms();
                let mode = *self.mode.lock().await;
                let dt = mode.dt();

                let (all_traces, tick_result) =
                    run_rr_tick(&mut state, &mut agents, &agent_names, dt as f64);
                self.current_tick.store(state.tick, Ordering::Relaxed);

                // Drain combat observations from the engine.
                let combat_observations = state.combat_log.drain();

                // Check for game over — all but one player eliminated.
                if !tick_result.eliminated.is_empty() {
                    let alive_count = (0..state.num_players)
                        .filter(|&p| {
                            state.entities.values().any(|e| {
                                e.owner == Some(p)
                                    && e.person.is_some()
                                    && e.vitals.as_ref().map(|v| v.blood > 0.0).unwrap_or(true)
                            })
                        })
                        .count();
                    if alive_count <= 1 {
                        game_over = true;
                    }
                }

                // Record tick for review system with real traces + combat log.
                {
                    let mut review = self.review.lock().await;
                    review.record_tick(&state, dt, combat_observations, all_traces);
                    review.collect_ready_flags().await;
                }

                // Broadcast delta to spectators (only changed fields).
                let delta = delta_tracker.build_delta(&state, dt);
                self.broadcast(V3ServerToSpectator::SnapshotDelta { delta })
                    .await;

                // Cache a full snapshot for late-joining spectators.
                let full = v3_protocol::build_snapshot(&state, dt);
                {
                    let mut snap = self.snapshot.lock().await;
                    snap.latest_snapshot = Some(V3ServerToSpectator::Snapshot { snapshot: full });
                }
                self.broadcast_rr_status().await;

                // Throttle to target tick rate.
                let elapsed = tick_start.elapsed();
                let target = tokio::time::Duration::from_millis(tick_ms);
                if elapsed < target {
                    tokio::time::sleep(target - elapsed).await;
                }
            }

            if !aborted {
                // Determine winner.
                let winner = (0..state.num_players).find(|&p| {
                    state.entities.values().any(|e| {
                        e.owner == Some(p)
                            && e.person.is_some()
                            && e.vitals.as_ref().map(|v| v.blood > 0.0).unwrap_or(true)
                    })
                });
                let scores: Vec<u32> = (0..state.num_players)
                    .map(|p| {
                        state
                            .entities
                            .values()
                            .filter(|e| e.owner == Some(p) && e.person.is_some())
                            .count() as u32
                    })
                    .collect();

                info!(
                    "V3 RR game #{} done: winner={:?}, ticks={}",
                    game_number, winner, state.tick
                );

                self.broadcast(V3ServerToSpectator::GameEnd {
                    winner,
                    tick: state.tick,
                    timed_out: state.tick >= MAX_TICKS,
                    scores,
                })
                .await;
                self.broadcast_rr_status().await;
            } else {
                info!("V3 RR game #{} aborted (reset)", game_number);
            }

            // Finalize any pending reviews.
            {
                let mut review = self.review.lock().await;
                review.finalize_all().await;
            }

            seed += 1;

            if !aborted {
                // Inter-game delay.
                tokio::time::sleep(tokio::time::Duration::from_millis(INTER_GAME_DELAY_MS)).await;

                // If autoplay is off, wait for resume signal.
                if !self.get_autoplay() {
                    self.pause();
                    self.wait_if_paused().await;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Agent creation
// ---------------------------------------------------------------------------

/// Create agents for a game. Rotates through personality types.
fn create_agents(
    num_players: u8,
    game_number: u64,
) -> (Vec<LayeredAgent>, Vec<String>, Vec<String>) {
    let personalities = ["Spread", "Striker", "Turtle"];
    let mut agents = Vec::new();
    let mut names = Vec::new();
    let mut versions = Vec::new();

    for i in 0..num_players {
        let personality_idx = ((game_number as usize) + i as usize) % personalities.len();
        let personality = personalities[personality_idx];
        let name = format!("{}_v1", personality);

        let strategy: Box<dyn simulate_everything_engine::v3::agent::StrategyLayer> =
            match personality {
                "Spread" => Box::new(SpreadStrategy::new()),
                "Striker" => Box::new(StrikerStrategy::new()),
                "Turtle" => Box::new(TurtleStrategy::new()),
                _ => unreachable!(),
            };

        let agent = LayeredAgent::new(
            strategy,
            Box::new(SharedOperationsLayer::new()),
            Box::new(SharedTacticalLayer::new(DamageEstimateTable::from_physics())),
            i,
            50, // strategy every 50 ticks
            5,  // operations every 5 ticks
        );

        agents.push(agent);
        names.push(name);
        versions.push("v1".to_string());
    }

    (agents, names, versions)
}

fn run_rr_tick(
    state: &mut GameState,
    agents: &mut [LayeredAgent],
    agent_names: &[String],
    dt: f64,
) -> (Vec<(String, Vec<AgentTrace>)>, sim::TickResult) {
    let phase = sim::run_agent_phase(state, agents);
    let traces = phase
        .outputs
        .into_iter()
        .zip(agent_names.iter().cloned())
        .filter_map(|(output, name)| (!output.traces.is_empty()).then_some((name, output.traces)))
        .collect();
    let tick_result = sim::tick(state, dt);
    (traces, tick_result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use simulate_everything_engine::v2::state::EntityKey;
    use simulate_everything_engine::v3::{
        agent::{
            OperationalCommand, OperationsLayer, StrategicDirective, StrategyLayer,
            TacticalCommand, TacticalLayer,
        },
        formation::FormationType,
        mapgen,
        perception::StrategicView,
        spatial::Vec3,
        state::{GameState, Stack},
    };

    struct NoopStrategy;

    impl StrategyLayer for NoopStrategy {
        fn plan(&mut self, _view: &StrategicView) -> Vec<StrategicDirective> {
            Vec::new()
        }
    }

    struct RouteOperations {
        stack: simulate_everything_engine::v3::state::StackId,
        destination: Vec3,
    }

    impl OperationsLayer for RouteOperations {
        fn execute(
            &mut self,
            _state: &GameState,
            _directives: &[StrategicDirective],
            _player: u8,
        ) -> Vec<OperationalCommand> {
            vec![OperationalCommand::RouteStack {
                stack: self.stack,
                waypoints: vec![self.destination],
            }]
        }
    }

    struct NoopTactical;

    impl TacticalLayer for NoopTactical {
        fn decide(
            &mut self,
            _state: &GameState,
            _stack: &Stack,
            _player: u8,
        ) -> Vec<TacticalCommand> {
            Vec::new()
        }
    }

    struct TraceAttackTactical {
        attacker: EntityKey,
        target: EntityKey,
    }

    impl TacticalLayer for TraceAttackTactical {
        fn decide(
            &mut self,
            _state: &GameState,
            _stack: &Stack,
            _player: u8,
        ) -> Vec<TacticalCommand> {
            vec![TacticalCommand::Attack {
                attacker: self.attacker,
                target: self.target,
            }]
        }
    }

    #[test]
    fn rr_tick_applies_tactical_commands_and_keeps_traces() {
        let mut state = mapgen::generate(15, 15, 2, 42);

        let attacker = state
            .entities
            .iter()
            .find_map(|(key, entity)| {
                (entity.owner == Some(0)
                    && entity.person.is_some()
                    && entity.mobile.is_some()
                    && entity.combatant.is_some())
                .then_some(key)
            })
            .expect("player 0 should own a combat-capable entity");
        let target = state
            .entities
            .iter()
            .find_map(|(key, entity)| {
                (entity.owner == Some(1)
                    && entity.person.is_some()
                    && entity.mobile.is_some()
                    && entity.combatant.is_some())
                .then_some(key)
            })
            .expect("player 1 should own a combat-capable entity");
        let attacker_pos = state.entities[attacker].pos.unwrap();
        state.entities[target].pos = Some(Vec3::new(
            attacker_pos.x + 1.0,
            attacker_pos.y,
            attacker_pos.z,
        ));
        let stack_id = state.alloc_stack_id();
        let mut members = state
            .stacks
            .first()
            .map(|stack| stack.members.clone())
            .unwrap_or_default();
        members.clear();
        members.push(attacker);
        state.stacks.push(Stack {
            id: stack_id,
            owner: 0,
            members,
            formation: FormationType::Line,
            leader: attacker,
        });

        let mut agents = vec![LayeredAgent::new(
            Box::new(NoopStrategy),
            Box::new(RouteOperations {
                stack: stack_id,
                destination: attacker_pos,
            }),
            Box::new(TraceAttackTactical { attacker, target }),
            0,
            1,
            1,
        )];
        let agent_names = vec!["test_agent".to_string()];

        let (traces, tick_result) = run_rr_tick(&mut state, &mut agents, &agent_names, 1.0);

        assert_eq!(tick_result.eliminated.len(), 0);
        assert_eq!(state.tick, 1);
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].0, "test_agent");
        assert!(!traces[0].1.is_empty());
        assert!(
            state.entities[attacker]
                .combatant
                .as_ref()
                .and_then(|combatant| combatant.attack.as_ref())
                .is_some(),
            "rr tick should still apply tactical attack state"
        );
    }

    #[test]
    fn rr_tick_applies_route_commands_before_movement() {
        let mut state = mapgen::generate(15, 15, 2, 42);
        let mover = state
            .entities
            .iter()
            .find_map(|(key, entity)| {
                (entity.owner == Some(0)
                    && entity.mobile.is_some()
                    && entity.pos.is_some()
                    && entity.person.is_some())
                .then_some(key)
            })
            .expect("player 0 should have a mobile entity");
        let initial_pos = state.entities[mover].pos.unwrap();
        let stack_id = state.alloc_stack_id();
        let mut members = state
            .stacks
            .first()
            .map(|stack| stack.members.clone())
            .unwrap_or_default();
        members.clear();
        members.push(mover);
        state.stacks.push(Stack {
            id: stack_id,
            owner: 0,
            members,
            formation: FormationType::Line,
            leader: mover,
        });

        let mut agents = vec![LayeredAgent::new(
            Box::new(NoopStrategy),
            Box::new(RouteOperations {
                stack: stack_id,
                destination: Vec3::new(initial_pos.x + 100.0, initial_pos.y, initial_pos.z),
            }),
            Box::new(NoopTactical),
            0,
            1,
            1,
        )];
        let agent_names = vec!["test_agent".to_string()];

        let (_traces, _tick_result) = run_rr_tick(&mut state, &mut agents, &agent_names, 1.0);

        let final_pos = state.entities[mover].pos.unwrap();
        assert!(
            final_pos.x > initial_pos.x,
            "entity should move after route command is applied before movement"
        );
    }
}
