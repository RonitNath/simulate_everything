use simulate_everything_engine::v2::{
    AGENT_POLL_INTERVAL, TIMEOUT_TICKS,
    agent::{Agent, SpreadAgent, StrikerAgent},
    directive,
    mapgen::{self, MapConfig},
    observation::{self, ObservationSession},
    sim, spectator,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::{Mutex, Notify, broadcast};
use tracing::info;

use crate::v2_protocol::V2ServerToSpectator;

#[derive(Debug, Clone, Default)]
pub struct V2RrSnapshot {
    pub init: Option<V2ServerToSpectator>,
    pub latest_snapshot: Option<V2ServerToSpectator>,
}

pub struct V2RoundRobin {
    pub tick_ms: AtomicU64,
    spectator_tx: broadcast::Sender<V2ServerToSpectator>,
    snapshot: Arc<Mutex<V2RrSnapshot>>,
    paused: AtomicBool,
    resume_notify: Notify,
    reset_flag: AtomicBool,
}

impl V2RoundRobin {
    pub fn new(tick_ms: u64) -> Self {
        let (spectator_tx, _) = broadcast::channel(512);
        Self {
            tick_ms: AtomicU64::new(tick_ms),
            spectator_tx,
            snapshot: Arc::new(Mutex::new(V2RrSnapshot::default())),
            paused: AtomicBool::new(false),
            resume_notify: Notify::new(),
            reset_flag: AtomicBool::new(false),
        }
    }

    pub fn set_tick_ms(&self, ms: u64) {
        let clamped = ms.clamp(10, 5000);
        self.tick_ms.store(clamped, Ordering::Relaxed);
        info!("V2 RR tick speed set to {}ms", clamped);
    }

    pub fn get_tick_ms(&self) -> u64 {
        self.tick_ms.load(Ordering::Relaxed)
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
        info!("V2 RR paused");
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
        self.resume_notify.notify_one();
        info!("V2 RR resumed");
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    pub fn reset(&self) {
        self.reset_flag.store(true, Ordering::Relaxed);
        self.paused.store(false, Ordering::Relaxed);
        self.resume_notify.notify_one();
        info!("V2 RR reset requested");
    }

    pub fn spectator_subscribe(&self) -> broadcast::Receiver<V2ServerToSpectator> {
        self.spectator_tx.subscribe()
    }

    pub async fn spectator_catchup(&self) -> Vec<V2ServerToSpectator> {
        let snap = self.snapshot.lock().await;
        let mut msgs = Vec::new();
        if let Some(ref init) = snap.init {
            msgs.push(init.clone());
        }
        if let Some(ref snapshot) = snap.latest_snapshot {
            msgs.push(snapshot.clone());
        }
        msgs
    }

    async fn cache_snapshot(&self, msg: V2ServerToSpectator) {
        let mut snap = self.snapshot.lock().await;
        if let V2ServerToSpectator::Snapshot { .. } = &msg {
            snap.latest_snapshot = Some(msg);
        }
    }

    async fn broadcast(&self, msg: V2ServerToSpectator) {
        let mut snap = self.snapshot.lock().await;
        match &msg {
            V2ServerToSpectator::Init { .. } => {
                snap.init = Some(msg.clone());
                snap.latest_snapshot = None;
            }
            V2ServerToSpectator::Snapshot { .. } => {
                snap.latest_snapshot = Some(msg.clone());
            }
            V2ServerToSpectator::GameEnd { .. } => {
                snap.init = None;
                snap.latest_snapshot = None;
            }
            V2ServerToSpectator::Config { .. } => {}
        }
        drop(snap);
        let _ = self.spectator_tx.send(msg);
    }

    pub async fn broadcast_config(&self, tick_ms: Option<u64>) {
        self.broadcast(V2ServerToSpectator::Config { tick_ms })
            .await;
    }

    async fn wait_if_paused(&self) {
        while self.paused.load(Ordering::Relaxed) {
            self.resume_notify.notified().await;
        }
    }

    fn take_reset(&self) -> bool {
        self.reset_flag.swap(false, Ordering::Relaxed)
    }

    pub async fn run_loop(self: Arc<Self>) {
        let mut seed: u64 = 1000;

        loop {
            if self.take_reset() {
                info!("V2 RR reset");
            }

            self.wait_if_paused().await;

            let config = MapConfig {
                width: 30,
                height: 30,
                num_players: 2,
                seed,
            };

            let game_number = seed - 999;
            let mut agents: Vec<Box<dyn Agent>> =
                vec![Box::new(SpreadAgent::new()), Box::new(StrikerAgent::new())];
            let agent_names: Vec<String> = agents
                .iter()
                .enumerate()
                .map(|(i, a)| format!("{} #{}", a.name(), i + 1))
                .collect();

            let mut state = mapgen::generate(&config);
            let mut session = ObservationSession::new(state.players.len(), state.width * state.height);
            for (pid, agent) in agents.iter_mut().enumerate() {
                let init = observation::initial_observation(&state, pid as u8);
                agent.reset();
                agent.init(&init);
            }

            info!(
                "V2 RR game #{}: {} (seed={})",
                game_number,
                agent_names.join(", "),
                seed
            );

            self.broadcast(V2ServerToSpectator::Init {
                init: spectator::spectator_init(&state, agent_names.clone()),
                game_number,
            })
            .await;

            let full_snapshot = spectator::snapshot(&state);
            self.broadcast(V2ServerToSpectator::Snapshot {
                snapshot: full_snapshot.clone(),
            })
            .await;
            self.cache_snapshot(V2ServerToSpectator::Snapshot {
                snapshot: full_snapshot,
            })
            .await;

            let max_ticks: u64 = TIMEOUT_TICKS;
            let mut aborted = false;

            while state.tick < max_ticks && !sim::is_over(&state) {
                if self.reset_flag.load(Ordering::Relaxed) {
                    aborted = true;
                    break;
                }

                self.wait_if_paused().await;

                let tick_start = tokio::time::Instant::now();
                let tick_ms = self.get_tick_ms();

                if state.tick % AGENT_POLL_INTERVAL as u64 == 0 {
                    for (player_id, agent) in agents.iter_mut().enumerate() {
                        let pid = player_id as u8;
                        if !state.players.iter().any(|p| p.id == pid && p.alive) {
                            continue;
                        }
                        let delta = observation::observe_delta(&mut state, pid, &mut session);
                        let directives = agent.act(&delta);
                        directive::apply_directives(&mut state, pid, &directives);
                    }
                    state.clear_dirty_hexes();
                }

                sim::tick(&mut state);

                self.broadcast(V2ServerToSpectator::Snapshot {
                    snapshot: spectator::snapshot_delta(&state),
                })
                .await;
                self.cache_snapshot(V2ServerToSpectator::Snapshot {
                    snapshot: spectator::snapshot(&state),
                })
                .await;
                state.clear_dirty_hexes();

                let compute_elapsed = tick_start.elapsed();
                let target = tokio::time::Duration::from_millis(tick_ms);
                if compute_elapsed < target {
                    tokio::time::sleep(target - compute_elapsed).await;
                }
            }

            if !aborted {
                let winner = sim::winner_at_limit(&state, max_ticks);
                info!("V2 RR game done: winner={:?}, ticks={}", winner, state.tick);

                self.broadcast(V2ServerToSpectator::GameEnd {
                    winner,
                    tick: state.tick,
                    timed_out: sim::reached_timeout(&state, max_ticks),
                })
                .await;
            } else {
                info!("V2 RR game aborted (reset)");
            }

            seed += 1;
            if !aborted {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }
    }
}
