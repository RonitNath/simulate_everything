use rand::{Rng, SeedableRng, rngs::StdRng};
use simulate_everything_engine::{
    agent::{Agent, rr_agents},
    event::PlayerStats,
    game::Game,
    mapgen::{self, MapConfig},
    replay::Frame,
    scoreboard::Scoreboard,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::{Mutex, Notify, broadcast};
use tracing::info;

use crate::protocol::ServerToSpectator;

#[derive(Debug, Clone, Default)]
pub struct RrSnapshot {
    pub game_start: Option<ServerToSpectator>,
    pub latest_frame: Option<ServerToSpectator>,
}

/// Rolling tick health metrics.
#[derive(Debug)]
pub struct TickHealth {
    /// Last N compute times in microseconds (before sleep).
    samples: Vec<u64>,
    head: usize,
    capacity: usize,
}

impl TickHealth {
    fn new(capacity: usize) -> Self {
        Self {
            samples: vec![0; capacity],
            head: 0,
            capacity,
        }
    }

    fn record(&mut self, compute_us: u64) {
        self.samples[self.head] = compute_us;
        self.head = (self.head + 1) % self.capacity;
    }

    fn avg_us(&self) -> u64 {
        let sum: u64 = self.samples.iter().sum();
        sum / self.capacity as u64
    }

    fn max_us(&self) -> u64 {
        *self.samples.iter().max().unwrap_or(&0)
    }

    fn overruns(&self, target_us: u64) -> usize {
        self.samples.iter().filter(|&&s| s > target_us).count()
    }
}

pub struct RoundRobin {
    pub tick_ms: AtomicU64,
    pub scoreboard: Arc<Mutex<Scoreboard>>,
    spectator_tx: broadcast::Sender<ServerToSpectator>,
    snapshot: Arc<Mutex<RrSnapshot>>,
    paused: AtomicBool,
    resume_notify: Notify,
    reset_flag: AtomicBool,
    health: Mutex<TickHealth>,
}

impl RoundRobin {
    pub fn new(tick_ms: u64, scoreboard: Arc<Mutex<Scoreboard>>) -> Self {
        let (spectator_tx, _) = broadcast::channel(512);
        Self {
            tick_ms: AtomicU64::new(tick_ms),
            scoreboard,
            spectator_tx,
            snapshot: Arc::new(Mutex::new(RrSnapshot::default())),
            paused: AtomicBool::new(false),
            resume_notify: Notify::new(),
            reset_flag: AtomicBool::new(false),
            health: Mutex::new(TickHealth::new(100)),
        }
    }

    pub fn set_tick_ms(&self, ms: u64) {
        let clamped = ms.clamp(10, 5000);
        self.tick_ms.store(clamped, Ordering::Relaxed);
        info!("RR tick speed set to {}ms", clamped);
    }

    pub fn get_tick_ms(&self) -> u64 {
        self.tick_ms.load(Ordering::Relaxed)
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
        info!("RR paused");
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
        self.resume_notify.notify_one();
        info!("RR resumed");
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn reset(&self) {
        self.reset_flag.store(true, Ordering::Relaxed);
        // Also unpause so the loop can see the reset flag.
        self.paused.store(false, Ordering::Relaxed);
        self.resume_notify.notify_one();
        info!("RR reset requested");
    }

    /// Returns (avg_compute_ms, max_compute_ms, overrun_pct, target_ms).
    pub async fn health_stats(&self) -> (f64, f64, f64, u64) {
        let h = self.health.lock().await;
        let target_ms = self.get_tick_ms();
        let target_us = target_ms * 1000;
        let avg_ms = h.avg_us() as f64 / 1000.0;
        let max_ms = h.max_us() as f64 / 1000.0;
        let overrun_pct = (h.overruns(target_us) as f64 / h.capacity as f64) * 100.0;
        (avg_ms, max_ms, overrun_pct, target_ms)
    }

    pub async fn broadcast_config(&self, show_numbers: Option<bool>, tick_ms: Option<u64>) {
        self.broadcast(ServerToSpectator::Config {
            show_numbers,
            tick_ms,
        })
        .await;
    }

    pub fn spectator_subscribe(&self) -> broadcast::Receiver<ServerToSpectator> {
        self.spectator_tx.subscribe()
    }

    pub async fn spectator_catchup(&self) -> Vec<ServerToSpectator> {
        let snap = self.snapshot.lock().await;
        let mut msgs = Vec::new();
        if let Some(ref gs) = snap.game_start {
            msgs.push(gs.clone());
        }
        if let Some(ref frame) = snap.latest_frame {
            msgs.push(frame.clone());
        }
        msgs
    }

    async fn broadcast(&self, msg: ServerToSpectator) {
        let mut snap = self.snapshot.lock().await;
        match &msg {
            ServerToSpectator::GameStart { .. } => {
                snap.game_start = Some(msg.clone());
                snap.latest_frame = None;
            }
            ServerToSpectator::Frame { .. } => {
                snap.latest_frame = Some(msg.clone());
            }
            ServerToSpectator::GameEnd { .. } => {
                snap.game_start = None;
                snap.latest_frame = None;
            }
            ServerToSpectator::Config { .. } => {}
            _ => {}
        }
        drop(snap);
        let _ = self.spectator_tx.send(msg);
    }

    /// Wait while paused. Returns immediately if not paused.
    async fn wait_if_paused(&self) {
        while self.paused.load(Ordering::Relaxed) {
            self.resume_notify.notified().await;
        }
    }

    /// Check if a reset was requested. Clears the flag.
    fn take_reset(&self) -> bool {
        self.reset_flag.swap(false, Ordering::Relaxed)
    }

    pub async fn run_loop(self: Arc<Self>) {
        let mut seed: u64 = 1000;

        loop {
            // Check for reset (clears scoreboard).
            if self.take_reset() {
                let mut sb = self.scoreboard.lock().await;
                *sb = Scoreboard::new();
                info!("RR scoreboard reset");
            }

            self.wait_if_paused().await;

            const NUM_PLAYERS: u8 = 2;

            let pool = rr_agents();
            if pool.is_empty() {
                info!("No built-in agents for round-robin");
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                continue;
            }

            let mut rng = StdRng::seed_from_u64(seed);

            // Sample NUM_PLAYERS agents from the pool with replacement.
            let mut agents: Vec<Box<dyn Agent>> = (0..NUM_PLAYERS)
                .map(|_| {
                    let idx = rng.gen_range(0..pool.len());
                    // Rebuild fresh instance each time.
                    rr_agents().swap_remove(idx)
                })
                .collect();

            // Disambiguate duplicate names with a suffix.
            let mut name_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            let agent_names: Vec<String> = agents
                .iter()
                .map(|a| {
                    let base = a.id();
                    let count = name_counts.entry(base.clone()).or_insert(0);
                    *count += 1;
                    if *count == 1 {
                        base
                    } else {
                        format!("{} #{}", base, count)
                    }
                })
                .collect();
            let agent_ids = agent_names.clone();

            let config = MapConfig::for_size(23, 23, NUM_PLAYERS);
            let state = mapgen::generate(&config, &mut rng);

            info!(
                "RR game #{}: {} (seed={})",
                seed - 999,
                agent_ids.join(", "),
                seed
            );

            self.broadcast(ServerToSpectator::GameStart {
                width: state.width,
                height: state.height,
                num_players: NUM_PLAYERS,
                agent_names: agent_names.clone(),
            })
            .await;

            let initial_frame = make_frame(&state);
            let zero_compute = vec![0u64; NUM_PLAYERS as usize];
            self.broadcast(ServerToSpectator::Frame {
                frame: initial_frame,
                compute_us: zero_compute,
            })
            .await;

            let mut game = Game::with_seed(state, 500, seed);
            let mut aborted = false;

            while !game.is_over() {
                // Check reset mid-game.
                if self.reset_flag.load(Ordering::Relaxed) {
                    aborted = true;
                    break;
                }

                self.wait_if_paused().await;

                let tick_start = tokio::time::Instant::now();
                let tick_ms = self.get_tick_ms();

                let observations = game.observations();
                let mut orders = Vec::new();
                let mut compute_us = Vec::with_capacity(agents.len());
                for (i, agent) in agents.iter_mut().enumerate() {
                    let t0 = std::time::Instant::now();
                    let actions = agent.act(&observations[i], &mut rng);
                    compute_us.push(t0.elapsed().as_micros() as u64);
                    orders.push((i as u8, actions));
                }

                game.step(&orders);

                let frame = make_frame(&game.state);
                self.broadcast(ServerToSpectator::Frame { frame, compute_us })
                    .await;

                let compute_elapsed = tick_start.elapsed();
                {
                    let mut h = self.health.lock().await;
                    h.record(compute_elapsed.as_micros() as u64);
                }

                let target = tokio::time::Duration::from_millis(tick_ms);
                if compute_elapsed < target {
                    tokio::time::sleep(target - compute_elapsed).await;
                }
            }

            if !aborted {
                let winner = game.state.winner;
                let turns = game.state.turn;

                info!(
                    "RR game done: winner={:?}, turns={}",
                    winner.map(|w| &agent_ids[w as usize]),
                    turns
                );

                {
                    let mut sb = self.scoreboard.lock().await;
                    sb.record(&agent_ids, winner.map(|w| w as usize));
                }

                self.broadcast(ServerToSpectator::GameEnd { winner, turns })
                    .await;
            } else {
                info!("RR game aborted (reset)");
            }

            seed += 1;
            if !aborted {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }
    }
}

fn make_frame(state: &simulate_everything_engine::GameState) -> Frame {
    let stats = (0..state.num_players)
        .map(|p| PlayerStats {
            player: p,
            land: state.land_count(p),
            armies: state.army_count(p),
            alive: state.alive[p as usize],
        })
        .collect();
    Frame {
        turn: state.turn,
        grid: state.grid.clone(),
        stats,
    }
}
