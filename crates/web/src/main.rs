mod lobby;
mod protocol;
mod roundrobin;
mod v2_protocol;
mod v2_roundrobin;
mod v2_rr_review;

use askama::Template;
use axum::{
    Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::get,
};
use lobby::{Lobby, TurnSubmission};
use protocol::{AgentToServer, ServerToAgent, SpectatorToServer};
use rand::{SeedableRng, rngs::StdRng};
use roundrobin::RoundRobin;
use serde::Deserialize;
use simulate_everything_engine::{
    agent::Agent,
    game::Game,
    mapgen::{self, MapConfig},
    replay::Replay,
    scoreboard::Scoreboard,
    v2,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{Mutex, broadcast};
use tower_http::services::ServeDir;
use tracing::{info, warn};
use v2_roundrobin::V2RoundRobin;

// ============================================================
// Shared state
// ============================================================

struct AppState {
    lobby: Arc<Lobby>,
    rr: Arc<RoundRobin>,
    v2_rr: Arc<V2RoundRobin>,
    scoreboard: Arc<Mutex<Scoreboard>>,
    build_ver: String,
}

// ============================================================
// Simulator (pregenerated replay)
// ============================================================

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    build_ver: String,
}

#[derive(Deserialize)]
struct GameParams {
    seed: Option<u64>,
    players: Option<u8>,
    turns: Option<u32>,
    width: Option<usize>,
    height: Option<usize>,
}

fn run_game(seed: u64, num_players: u8, max_turns: u32, size: Option<(usize, usize)>) -> Replay {
    use rand::seq::SliceRandom;
    use simulate_everything_engine::agent::all_builtin_agents;

    let mut rng = StdRng::seed_from_u64(seed);
    let config = match size {
        Some((w, h)) => MapConfig::for_size(w, h, num_players),
        None => MapConfig::for_players(num_players),
    };
    let state = mapgen::generate(&config, &mut rng);

    let mut pool = all_builtin_agents();
    pool.shuffle(&mut rng);
    let mut agents: Vec<Box<dyn Agent>> = pool.into_iter().take(num_players as usize).collect();

    let agent_names: Vec<String> = agents.iter().map(|a| a.id()).collect();
    let mut replay = Replay::new(&state, agent_names);
    let mut game = Game::with_seed(state, max_turns, seed);

    while !game.is_over() {
        let observations = game.observations();
        let mut orders = Vec::new();
        for (i, agent) in agents.iter_mut().enumerate() {
            let actions = agent.act(&observations[i], &mut rng);
            orders.push((i as u8, actions));
        }
        game.step(&orders);
        replay.capture(&game.state);
    }
    replay.finalize(&game.state);
    replay
}

async fn simulator_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Html(
        IndexTemplate {
            build_ver: state.build_ver.clone(),
        }
        .render()
        .unwrap(),
    )
}

async fn api_game(Query(params): Query<GameParams>) -> impl IntoResponse {
    let seed = params.seed.unwrap_or_else(|| rand::random());
    let players = params.players.unwrap_or(2);
    let turns = params.turns.unwrap_or(500);
    let size = match (params.width, params.height) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    };

    Json(run_game(seed, players, turns, size))
}

/// ASCII screenshot of a simulated game at a specific turn (or final state).
/// GET /api/ascii?seed=42&players=2&turns=500&at=100
#[derive(Deserialize)]
struct AsciiParams {
    seed: Option<u64>,
    players: Option<u8>,
    turns: Option<u32>,
    width: Option<usize>,
    height: Option<usize>,
    /// Which turn to snapshot. If omitted, returns the final state.
    at: Option<u32>,
}

async fn api_ascii(Query(params): Query<AsciiParams>) -> impl IntoResponse {
    let seed = params.seed.unwrap_or(42);
    let num_players = params.players.unwrap_or(2);
    let max_turns = params.turns.unwrap_or(500);
    let size = match (params.width, params.height) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    };

    let replay = run_game(seed, num_players, max_turns, size);

    let frame = match params.at {
        Some(turn) => replay
            .frames
            .iter()
            .find(|f| f.turn == turn)
            .or_else(|| replay.frames.last()),
        None => replay.frames.last(),
    };

    match frame {
        Some(f) => f.ascii(replay.width, replay.height).to_string(),
        None => "No frames captured".to_string(),
    }
}

// ============================================================
// V2 Simulator
// ============================================================

#[derive(Deserialize)]
struct V2GameParams {
    seed: Option<u64>,
    players: Option<u8>,
    width: Option<usize>,
    height: Option<usize>,
    ticks: Option<u64>,
}

async fn api_v2_game(Query(params): Query<V2GameParams>) -> impl IntoResponse {
    let config = v2::mapgen::MapConfig {
        width: params.width.unwrap_or(30),
        height: params.height.unwrap_or(30),
        num_players: params.players.unwrap_or(2),
        seed: params.seed.unwrap_or_else(|| rand::random()),
    };
    let max_ticks = params.ticks.unwrap_or(2000);
    let replay = tokio::task::spawn_blocking(move || {
        let mut agents: Vec<Box<dyn v2::agent::Agent>> = (0..config.num_players)
            .map(|_| Box::new(v2::agent::SpreadAgent::new()) as Box<dyn v2::agent::Agent>)
            .collect();
        v2::replay::record_game(&config, &mut agents, max_ticks, 10)
    })
    .await
    .unwrap();
    Json(replay)
}

#[derive(Deserialize)]
struct V2AsciiParams {
    seed: Option<u64>,
    players: Option<u8>,
    width: Option<usize>,
    height: Option<usize>,
    ticks: Option<u64>,
    at: Option<u64>,
}

async fn api_v2_ascii(Query(params): Query<V2AsciiParams>) -> impl IntoResponse {
    let config = v2::mapgen::MapConfig {
        width: params.width.unwrap_or(30),
        height: params.height.unwrap_or(30),
        num_players: params.players.unwrap_or(2),
        seed: params.seed.unwrap_or(42),
    };
    let max_ticks = params.ticks.unwrap_or(2000);
    let at = params.at;
    tokio::task::spawn_blocking(move || {
        let mut agents: Vec<Box<dyn v2::agent::Agent>> = (0..config.num_players)
            .map(|_| Box::new(v2::agent::SpreadAgent::new()) as Box<dyn v2::agent::Agent>)
            .collect();
        let replay = v2::replay::record_game(&config, &mut agents, max_ticks, 10);

        let frame = match at {
            Some(at) => replay
                .frames
                .iter()
                .min_by_key(|f| (f.tick as i64 - at as i64).unsigned_abs())
                .or(replay.frames.last()),
            None => replay.frames.last(),
        };

        match frame {
            Some(f) => {
                let state = v2::replay::reconstruct_state(&replay, f);
                v2::ascii::render_state(&state)
            }
            None => "No frames captured".to_string(),
        }
    })
    .await
    .unwrap()
}

// ============================================================
// Live game server
// ============================================================

#[derive(Template)]
#[template(path = "live.html")]
struct LiveTemplate {
    build_ver: String,
}

async fn live_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Html(
        LiveTemplate {
            build_ver: state.build_ver.clone(),
        }
        .render()
        .unwrap(),
    )
}

async fn ws_agent(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_agent(socket, state))
}

async fn handle_agent(mut socket: WebSocket, state: Arc<AppState>) {
    let name = match wait_for_join(&mut socket).await {
        Some(name) => name,
        None => return,
    };

    let (slot, mut obs_rx, action_tx) = match state.lobby.add_agent(name.clone()).await {
        Ok(v) => v,
        Err(e) => {
            let msg = serde_json::to_string(&ServerToAgent::Error { message: e }).unwrap();
            let _ = socket.send(Message::Text(msg.into())).await;
            return;
        }
    };

    info!("Agent '{}' connected as player {}", name, slot);

    let name_clone = name.clone();
    loop {
        tokio::select! {
            Some(msg) = obs_rx.recv() => {
                let text = serde_json::to_string(&msg).unwrap();
                if socket.send(Message::Text(text.into())).await.is_err() {
                    info!("Agent '{}' send failed, disconnecting", name_clone);
                    break;
                }
            }
            ws_msg = socket.recv() => {
                match ws_msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<AgentToServer>(&text) {
                            Ok(AgentToServer::Actions { actions }) => {
                                let _ = action_tx.send(TurnSubmission { actions }).await;
                            }
                            Ok(AgentToServer::Join { .. }) => {}
                            Err(e) => {
                                warn!("Agent '{}' invalid message: {}", name_clone, e);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        info!("Agent '{}' disconnected", name_clone);
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    state.lobby.remove_agent(slot).await;
}

async fn wait_for_join(socket: &mut WebSocket) -> Option<String> {
    let deadline = tokio::time::Duration::from_secs(10);
    match tokio::time::timeout(deadline, socket.recv()).await {
        Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<AgentToServer>(&text) {
            Ok(AgentToServer::Join { name }) => Some(name),
            _ => {
                warn!("Expected Join message, got: {}", text);
                None
            }
        },
        _ => {
            warn!("Agent failed to send Join message in time");
            None
        }
    }
}

// ============================================================
// Spectator WebSocket handler (reusable for live + rr)
// ============================================================

async fn ws_spectate_live(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let rx = state.lobby.spectator_subscribe();
    let catchup = state.lobby.spectator_catchup().await;
    let lobby = state.lobby.clone();
    let build_ver = state.build_ver.clone();
    ws.on_upgrade(move |socket| {
        handle_spectator(socket, rx, catchup, SpeedTarget::Lobby(lobby), build_ver)
    })
}

async fn ws_spectate_rr(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let rx = state.rr.spectator_subscribe();
    let catchup = state.rr.spectator_catchup().await;
    let rr = state.rr.clone();
    let build_ver = state.build_ver.clone();
    ws.on_upgrade(move |socket| {
        handle_spectator(socket, rx, catchup, SpeedTarget::Rr(rr), build_ver)
    })
}

enum SpeedTarget {
    Lobby(Arc<Lobby>),
    Rr(Arc<RoundRobin>),
}

async fn handle_spectator(
    mut socket: WebSocket,
    mut rx: tokio::sync::broadcast::Receiver<protocol::ServerToSpectator>,
    catchup: Vec<protocol::ServerToSpectator>,
    speed_target: SpeedTarget,
    build_ver: String,
) {
    // Send build version first so clients can detect stale JS and reload.
    let hello = serde_json::json!({ "type": "hello", "build_ver": build_ver });
    if socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    info!(
        "Spectator connected, sending {} catchup messages",
        catchup.len()
    );
    for msg in &catchup {
        let label = match msg {
            protocol::ServerToSpectator::GameStart { .. } => "game_start",
            protocol::ServerToSpectator::Frame { .. } => "frame",
            protocol::ServerToSpectator::GameEnd { .. } => "game_end",
            protocol::ServerToSpectator::Lobby { .. } => "lobby",
            protocol::ServerToSpectator::Config { .. } => "config",
        };
        info!("  catchup: {}", label);
    }
    for msg in catchup {
        let text = serde_json::to_string(&msg).unwrap();
        if socket.send(Message::Text(text.into())).await.is_err() {
            return;
        }
    }

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        let text = serde_json::to_string(&msg).unwrap();
                        if socket.send(Message::Text(text.into())).await.is_err() { break; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Spectator lagged, dropped {} messages", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            ws_msg = socket.recv() => {
                match ws_msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(msg) = serde_json::from_str::<SpectatorToServer>(&text) {
                            match msg {
                                SpectatorToServer::SetSpeed { tick_ms } => {
                                    match &speed_target {
                                        SpeedTarget::Lobby(l) => l.set_tick_ms(tick_ms),
                                        SpeedTarget::Rr(r) => r.set_tick_ms(tick_ms),
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

// ============================================================
// Round-Robin
// ============================================================

#[derive(Template)]
#[template(path = "rr.html")]
struct RrTemplate {
    build_ver: String,
}

async fn rr_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Html(
        RrTemplate {
            build_ver: state.build_ver.clone(),
        }
        .render()
        .unwrap(),
    )
}

// ============================================================
// Scoreboard
// ============================================================

#[derive(Template)]
#[template(path = "scoreboard.html")]
struct ScoreboardTemplate {
    build_ver: String,
}

async fn scoreboard_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Html(
        ScoreboardTemplate {
            build_ver: state.build_ver.clone(),
        }
        .render()
        .unwrap(),
    )
}

async fn api_scoreboard(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sb = state.scoreboard.lock().await;
    Json(serde_json::json!({
        "total_games": sb.total_games,
        "agents": sb.ranked().iter().map(|r| serde_json::json!({
            "id": r.id,
            "wins": r.wins,
            "losses": r.losses,
            "draws": r.draws,
            "games": r.games(),
            "win_rate": format!("{:.1}", r.win_rate() * 100.0),
        })).collect::<Vec<_>>(),
    }))
}

// ============================================================
// Config API (CLI-controllable)
// ============================================================

#[derive(Deserialize)]
struct ConfigUpdate {
    show_numbers: Option<bool>,
    tick_ms: Option<u64>,
}

async fn api_rr_config(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<ConfigUpdate>,
) -> impl IntoResponse {
    if let Some(ms) = body.tick_ms {
        state.rr.set_tick_ms(ms);
    }
    let _ = state
        .rr
        .broadcast_config(body.show_numbers, body.tick_ms)
        .await;
    Json(serde_json::json!({"ok": true}))
}

async fn api_live_config(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<ConfigUpdate>,
) -> impl IntoResponse {
    if let Some(ms) = body.tick_ms {
        state.lobby.set_tick_ms(ms);
    }
    let _ = state
        .lobby
        .broadcast_config(body.show_numbers, body.tick_ms)
        .await;
    Json(serde_json::json!({"ok": true}))
}

/// POST /api/rr/pause
async fn api_rr_pause(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.rr.pause();
    Json(serde_json::json!({"ok": true, "paused": true}))
}

/// POST /api/rr/resume
async fn api_rr_resume(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.rr.resume();
    Json(serde_json::json!({"ok": true, "paused": false}))
}

/// POST /api/rr/reset — aborts current game, clears scoreboard, starts fresh
async fn api_rr_reset(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.rr.reset();
    Json(serde_json::json!({"ok": true, "reset": true}))
}

/// GET /api/rr/status
async fn api_rr_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let (avg_ms, max_ms, overrun_pct, target_ms) = state.rr.health_stats().await;
    Json(serde_json::json!({
        "paused": state.rr.is_paused(),
        "tick_ms": target_ms,
        "health": {
            "avg_compute_ms": format!("{:.1}", avg_ms),
            "max_compute_ms": format!("{:.1}", max_ms),
            "overrun_pct": format!("{:.0}", overrun_pct),
            "headroom_pct": format!("{:.0}", ((target_ms as f64 - avg_ms) / target_ms as f64 * 100.0).max(0.0)),
        }
    }))
}

// ============================================================
// V2 Pages
// ============================================================

#[derive(Template)]
#[template(path = "v2sim.html")]
struct V2SimTemplate {
    build_ver: String,
}

async fn v2_sim_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Html(
        V2SimTemplate {
            build_ver: state.build_ver.clone(),
        }
        .render()
        .unwrap(),
    )
}

#[derive(Template)]
#[template(path = "v2rr.html")]
struct V2RrTemplate {
    build_ver: String,
}

async fn v2_rr_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Html(
        V2RrTemplate {
            build_ver: state.build_ver.clone(),
        }
        .render()
        .unwrap(),
    )
}

async fn ws_v2_rr(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let rx = state.v2_rr.spectator_subscribe();
    let catchup = state.v2_rr.spectator_catchup().await;
    ws.on_upgrade(move |socket| handle_v2_spectator(socket, rx, catchup))
}

async fn handle_v2_spectator(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<v2_protocol::V2ServerToSpectator>,
    catchup: Vec<v2_protocol::V2ServerToSpectator>,
) {
    for msg in catchup {
        let text = serde_json::to_string(&msg).unwrap();
        if socket.send(Message::Text(text.into())).await.is_err() {
            return;
        }
    }

    loop {
        match rx.recv().await {
            Ok(msg) => {
                let text = serde_json::to_string(&msg).unwrap();
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!("V2 spectator lagged, dropped {} messages", n);
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

#[derive(Deserialize)]
struct V2RrConfigUpdate {
    tick_ms: Option<u64>,
}

#[derive(Deserialize)]
struct V2RrFlagRequest {
    game_number: u64,
    tick: u64,
}

async fn api_v2_rr_config(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<V2RrConfigUpdate>,
) -> impl IntoResponse {
    if let Some(ms) = body.tick_ms {
        state.v2_rr.set_tick_ms(ms);
    }
    let _ = state.v2_rr.broadcast_config(body.tick_ms).await;
    Json(serde_json::json!({"ok": true}))
}

async fn api_v2_rr_pause(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.v2_rr.pause();
    Json(serde_json::json!({"ok": true, "paused": true}))
}

async fn api_v2_rr_resume(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.v2_rr.resume();
    Json(serde_json::json!({"ok": true, "paused": false}))
}

async fn api_v2_rr_reset(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.v2_rr.reset();
    Json(serde_json::json!({"ok": true, "reset": true}))
}

async fn api_v2_rr_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let review = state.v2_rr.review_status().await;
    Json(serde_json::json!({
        "paused": state.v2_rr.is_paused(),
        "tick_ms": state.v2_rr.get_tick_ms(),
        "game_number": review.game_number,
        "current_tick": review.current_tick,
        "capturable_start_tick": review.capturable_start_tick,
        "capturable_end_tick": review.capturable_end_tick,
        "pending_capture_count": review.pending_capture_count,
        "active_capture": review.active_capture,
        "review_dir": review.review_dir,
    }))
}

async fn api_v2_rr_flag(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<V2RrFlagRequest>,
) -> impl IntoResponse {
    match state.v2_rr.flag_tick(body.game_number, body.tick).await {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "summary": result.summary,
                "capturable_start_tick": result.capturable_start_tick,
                "capturable_end_tick": result.capturable_end_tick,
            })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "ok": false, "error": err })),
        )
            .into_response(),
    }
}

async fn api_v2_rr_capture_start(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<V2RrFlagRequest>,
) -> impl IntoResponse {
    match state.v2_rr.start_capture(body.game_number, body.tick).await {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "summary": result.summary,
                "capturable_start_tick": result.capturable_start_tick,
                "capturable_end_tick": result.capturable_end_tick,
            })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "ok": false, "error": err })),
        )
            .into_response(),
    }
}

async fn api_v2_rr_capture_stop(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<V2RrFlagRequest>,
) -> impl IntoResponse {
    match state.v2_rr.stop_capture(body.game_number, body.tick).await {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "summary": result.summary,
                "capturable_start_tick": result.capturable_start_tick,
                "capturable_end_tick": result.capturable_end_tick,
            })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "ok": false, "error": err })),
        )
            .into_response(),
    }
}

async fn api_v2_rr_reviews(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.v2_rr.list_reviews().await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn api_v2_rr_review(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.v2_rr.load_review(&id).await {
        Ok(Some(bundle)) => (StatusCode::OK, Json(bundle)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "review bundle not found" })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn api_v2_rr_delete_review(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.v2_rr.delete_review(&id).await {
        Ok(true) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "review bundle not found" })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

// ============================================================
// Main
// ============================================================

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let static_dir = std::env::var("SIMEV_STATIC_DIR").unwrap_or_else(|_| {
        let manifest = env!("CARGO_MANIFEST_DIR");
        format!("{}/../../frontend/dist", manifest)
    });

    let num_players: u8 = std::env::var("SIMEV_PLAYERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let tick_ms: u64 = std::env::var("SIMEV_TICK_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(250);
    let seed: u64 = std::env::var("SIMEV_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(42);
    let review_dir = std::env::var("SIMEV_V2_RR_REVIEW_DIR").unwrap_or_else(|_| {
        let manifest = env!("CARGO_MANIFEST_DIR");
        format!("{}/../../var/v2_rr_reviews", manifest)
    });

    let scoreboard = Arc::new(Mutex::new(Scoreboard::new()));

    let lobby = Arc::new(Lobby::new(num_players, 500, tick_ms, seed));
    let lobby_loop = lobby.clone();
    tokio::spawn(async move {
        lobby_loop.run_loop().await;
    });

    let rr = Arc::new(RoundRobin::new(tick_ms, scoreboard.clone()));
    let rr_loop = rr.clone();
    tokio::spawn(async move {
        rr_loop.run_loop().await;
    });

    let v2_rr = Arc::new(V2RoundRobin::new(tick_ms, review_dir.into()));
    let v2_rr_loop = v2_rr.clone();
    tokio::spawn(async move {
        v2_rr_loop.run_loop().await;
    });

    let build_ver = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let state = Arc::new(AppState {
        lobby,
        rr,
        v2_rr,
        scoreboard,
        build_ver,
    });

    info!("Serving static files from: {}", static_dir);

    let app = Router::new()
        .route("/", get(v2_sim_page))
        .route("/rr", get(v2_rr_page))
        .route("/ws/rr", get(ws_v2_rr))
        .route("/api/game", get(api_game))
        .route("/api/ascii", get(api_ascii))
        .route("/api/v2/game", get(api_v2_game))
        .route("/api/v2/ascii", get(api_v2_ascii))
        .route("/live", get(live_page))
        .route("/ws/agent", get(ws_agent))
        .route("/ws/spectate", get(ws_spectate_live))
        .route("/v1", get(simulator_page))
        .route("/v1/rr", get(rr_page))
        .route("/ws/v1/rr", get(ws_spectate_rr))
        .route("/scoreboard", get(scoreboard_page))
        .route("/api/scoreboard", get(api_scoreboard))
        .route("/api/rr/config", axum::routing::post(api_rr_config))
        .route("/api/rr/pause", axum::routing::post(api_rr_pause))
        .route("/api/rr/resume", axum::routing::post(api_rr_resume))
        .route("/api/rr/reset", axum::routing::post(api_rr_reset))
        .route("/api/rr/status", get(api_rr_status))
        .route("/api/live/config", axum::routing::post(api_live_config))
        .route("/v2", get(v2_sim_page))
        .route("/v2/rr", get(v2_rr_page))
        .route("/ws/v2/rr", get(ws_v2_rr))
        .route("/api/v2/rr/config", axum::routing::post(api_v2_rr_config))
        .route("/api/v2/rr/pause", axum::routing::post(api_v2_rr_pause))
        .route("/api/v2/rr/resume", axum::routing::post(api_v2_rr_resume))
        .route("/api/v2/rr/reset", axum::routing::post(api_v2_rr_reset))
        .route("/api/v2/rr/flags", axum::routing::post(api_v2_rr_flag))
        .route("/api/v2/rr/capture/start", axum::routing::post(api_v2_rr_capture_start))
        .route("/api/v2/rr/capture/stop", axum::routing::post(api_v2_rr_capture_stop))
        .route("/api/v2/rr/reviews", get(api_v2_rr_reviews))
        .route(
            "/api/v2/rr/reviews/{id}",
            get(api_v2_rr_review).delete(api_v2_rr_delete_review),
        )
        .route("/api/v2/rr/status", get(api_v2_rr_status))
        .nest_service("/static", ServeDir::new(&static_dir))
        .with_state(state);

    let bind_addr: std::net::IpAddr = std::env::var("SIMEV_BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0".into())
        .parse()
        .expect("SIMEV_BIND_ADDR must be a valid IP address");
    let bind_port: u16 = std::env::var("SIMEV_PORT")
        .unwrap_or_else(|_| "3333".into())
        .parse()
        .expect("SIMEV_PORT must be a valid port number");
    let addr = SocketAddr::from((bind_addr, bind_port));
    info!("Listening on http://{}", addr);

    // Retry bind to handle port still held by a dying predecessor.
    let listener = loop {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => break l,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                tracing::warn!("Port {} in use, retrying in 500ms...", addr.port());
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            Err(e) => panic!("Failed to bind {}: {}", addr, e),
        }
    };
    axum::serve(listener, app).await.unwrap();
}
