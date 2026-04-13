use crate::protocol::*;
use generals_engine::{
    event::PlayerStats,
    game::Game,
    mapgen::{self, MapConfig},
    replay::Frame,
};
use rand::{rngs::StdRng, SeedableRng};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex, Notify};
use tracing::{info, warn};

pub struct TurnSubmission {
    pub actions: Vec<generals_engine::action::Action>,
}

struct AgentSlot {
    slot: u8,
    name: String,
    obs_tx: mpsc::Sender<ServerToAgent>,
    action_rx: Arc<Mutex<mpsc::Receiver<TurnSubmission>>>,
}

/// Cached state for late-joining spectators.
#[derive(Debug, Clone)]
pub struct SpectatorSnapshot {
    pub game_start: Option<ServerToSpectator>,
    pub latest_frame: Option<ServerToSpectator>,
    pub lobby: Option<ServerToSpectator>,
}

pub struct Lobby {
    pub num_players: u8,
    pub max_turns: u32,
    pub tick_ms: AtomicU64,
    seed: Arc<Mutex<u64>>,
    agents: Arc<Mutex<Vec<AgentSlot>>>,
    spectator_tx: broadcast::Sender<ServerToSpectator>,
    ready_notify: Arc<Notify>,
    /// Cached state for late-joining spectators.
    snapshot: Arc<Mutex<SpectatorSnapshot>>,
}

impl Lobby {
    pub fn new(num_players: u8, max_turns: u32, tick_ms: u64, seed: u64) -> Self {
        let (spectator_tx, _) = broadcast::channel(512);
        Self {
            num_players,
            max_turns,
            tick_ms: AtomicU64::new(tick_ms),
            seed: Arc::new(Mutex::new(seed)),
            agents: Arc::new(Mutex::new(Vec::new())),
            spectator_tx,
            ready_notify: Arc::new(Notify::new()),
            snapshot: Arc::new(Mutex::new(SpectatorSnapshot {
                game_start: None,
                latest_frame: None,
                lobby: None,
            })),
        }
    }

    pub fn set_tick_ms(&self, ms: u64) {
        let clamped = ms.clamp(10, 5000);
        self.tick_ms.store(clamped, Ordering::Relaxed);
        info!("Tick speed set to {}ms", clamped);
    }

    pub fn get_tick_ms(&self) -> u64 {
        self.tick_ms.load(Ordering::Relaxed)
    }

    pub async fn broadcast_config(&self, show_numbers: Option<bool>, tick_ms: Option<u64>) {
        self.broadcast(ServerToSpectator::Config { show_numbers, tick_ms }).await;
    }

    pub fn spectator_subscribe(&self) -> broadcast::Receiver<ServerToSpectator> {
        self.spectator_tx.subscribe()
    }

    /// Broadcast to spectators and cache for late joiners.
    async fn broadcast(&self, msg: ServerToSpectator) {
        // Update snapshot cache.
        let mut snap = self.snapshot.lock().await;
        match &msg {
            ServerToSpectator::Lobby { .. } => snap.lobby = Some(msg.clone()),
            ServerToSpectator::GameStart { .. } => {
                snap.game_start = Some(msg.clone());
                snap.latest_frame = None;
                snap.lobby = None;
            }
            ServerToSpectator::Frame { .. } => snap.latest_frame = Some(msg.clone()),
            ServerToSpectator::GameEnd { .. } => {
                snap.game_start = None;
                snap.latest_frame = None;
            }
            ServerToSpectator::Config { .. } => {}
        }
        drop(snap);
        let _ = self.spectator_tx.send(msg);
    }

    /// Get cached state for a late-joining spectator.
    pub async fn spectator_catchup(&self) -> Vec<ServerToSpectator> {
        let snap = self.snapshot.lock().await;
        let mut msgs = Vec::new();
        if let Some(ref lobby) = snap.lobby {
            msgs.push(lobby.clone());
        }
        if let Some(ref gs) = snap.game_start {
            msgs.push(gs.clone());
        }
        if let Some(ref frame) = snap.latest_frame {
            msgs.push(frame.clone());
        }
        msgs
    }

    pub async fn add_agent(
        &self,
        name: String,
    ) -> Result<(u8, mpsc::Receiver<ServerToAgent>, mpsc::Sender<TurnSubmission>), String> {
        let mut agents = self.agents.lock().await;
        if agents.len() >= self.num_players as usize {
            return Err("Lobby is full".to_string());
        }

        let slot = agents.len() as u8;
        let (obs_tx, obs_rx) = mpsc::channel(8);
        let (action_tx, action_rx) = mpsc::channel(8);

        agents.push(AgentSlot {
            slot,
            name: name.clone(),
            obs_tx,
            action_rx: Arc::new(Mutex::new(action_rx)),
        });

        let connected = agents.len() as u8;
        let needed = self.num_players;

        for agent in agents.iter() {
            let _ = agent.obs_tx.try_send(ServerToAgent::Lobby {
                slot: agent.slot,
                name: agent.name.clone(),
                players_connected: connected,
                players_needed: needed,
            });
        }

        let lobby_players: Vec<LobbyPlayer> = agents
            .iter()
            .map(|a| LobbyPlayer { slot: a.slot, name: a.name.clone() })
            .collect();
        self.broadcast(ServerToSpectator::Lobby {
            players: lobby_players,
            players_needed: needed,
        }).await;

        info!("Agent '{}' joined as player {} ({}/{})", name, slot, connected, needed);

        if connected == needed {
            self.ready_notify.notify_one();
        }

        Ok((slot, obs_rx, action_tx))
    }

    pub async fn remove_agent(&self, slot: u8) {
        let mut agents = self.agents.lock().await;
        agents.retain(|a| a.slot != slot);
        let connected = agents.len() as u8;
        info!("Player {} disconnected ({}/{} remaining)", slot, connected, self.num_players);

        let lobby_players: Vec<LobbyPlayer> = agents
            .iter()
            .map(|a| LobbyPlayer { slot: a.slot, name: a.name.clone() })
            .collect();
        self.broadcast(ServerToSpectator::Lobby {
            players: lobby_players,
            players_needed: self.num_players,
        }).await;
    }

    pub async fn run_loop(self: Arc<Self>) {
        loop {
            info!("Waiting for {} players...", self.num_players);
            loop {
                let count = self.agents.lock().await.len();
                if count >= self.num_players as usize {
                    break;
                }
                self.ready_notify.notified().await;
            }

            let seed = {
                let mut s = self.seed.lock().await;
                let current = *s;
                *s += 1;
                current
            };
            self.run_game(seed).await;

            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }
    }

    async fn run_game(&self, seed: u64) {
        let mut rng = StdRng::seed_from_u64(seed);
        let config = MapConfig::for_players(self.num_players);
        let state = mapgen::generate(&config, &mut rng);

        let agents = self.agents.lock().await;
        let agent_names: Vec<String> = agents.iter().map(|a| a.name.clone()).collect();

        info!(
            "Starting game: {}x{}, {} players, seed={}, agents={:?}",
            config.width, config.height, self.num_players, seed, agent_names
        );

        for agent in agents.iter() {
            let _ = agent.obs_tx.try_send(ServerToAgent::GameStart {
                player: agent.slot,
                width: state.width,
                height: state.height,
                num_players: self.num_players,
            });
        }

        self.broadcast(ServerToSpectator::GameStart {
            width: state.width,
            height: state.height,
            num_players: self.num_players,
            agent_names: agent_names.clone(),
        }).await;

        let initial_frame = make_frame(&state);
        let zero_compute = vec![0u64; self.num_players as usize];
        self.broadcast(ServerToSpectator::Frame { frame: initial_frame, compute_us: zero_compute }).await;

        let mut game = Game::with_seed(state, self.max_turns, seed);

        let action_rxs: Vec<Arc<Mutex<mpsc::Receiver<TurnSubmission>>>> =
            agents.iter().map(|a| a.action_rx.clone()).collect();
        let obs_txs: Vec<mpsc::Sender<ServerToAgent>> =
            agents.iter().map(|a| a.obs_tx.clone()).collect();

        drop(agents);

        while !game.is_over() {
            let tick_start = tokio::time::Instant::now();
            let tick_ms = self.get_tick_ms();
            let action_deadline = tokio::time::Duration::from_millis(tick_ms);

            // Send observations.
            let observations = game.observations();
            for (i, obs) in observations.into_iter().enumerate() {
                let _ = obs_txs[i].send(ServerToAgent::Observation { obs }).await;
            }

            // Collect actions with timeout.
            let mut orders: Vec<(u8, Vec<generals_engine::action::Action>)> =
                (0..self.num_players).map(|p| (p, vec![])).collect();

            let deadline = tokio::time::Instant::now() + action_deadline;

            for (i, rx) in action_rxs.iter().enumerate() {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    continue;
                }
                let mut rx_guard = rx.lock().await;
                match tokio::time::timeout(remaining, rx_guard.recv()).await {
                    Ok(Some(submission)) => {
                        orders[i].1 = submission.actions;
                    }
                    Ok(None) => {
                        warn!("Player {} disconnected during game", i);
                    }
                    Err(_) => {}
                }
            }

            game.step(&orders);

            let frame = make_frame(&game.state);
            let zero_compute = vec![0u64; self.num_players as usize];
            self.broadcast(ServerToSpectator::Frame { frame, compute_us: zero_compute }).await;

            // Enforce minimum tick pacing — sleep until tick_ms has elapsed.
            let elapsed = tick_start.elapsed();
            let tick_target = tokio::time::Duration::from_millis(tick_ms);
            if elapsed < tick_target {
                tokio::time::sleep(tick_target - elapsed).await;
            }
        }

        let winner = game.state.winner;
        let turns = game.state.turn;

        info!("Game over: winner={:?}, turns={}", winner, turns);

        let agents = self.agents.lock().await;
        for agent in agents.iter() {
            let _ = agent.obs_tx.try_send(ServerToAgent::GameEnd { winner, turns });
        }

        self.broadcast(ServerToSpectator::GameEnd { winner, turns }).await;
    }
}

fn make_frame(state: &generals_engine::GameState) -> Frame {
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
