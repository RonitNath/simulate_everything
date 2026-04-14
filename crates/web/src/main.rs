mod lobby;
mod protocol;
mod roundrobin;
mod v2_protocol;
mod v2_roundrobin;
mod v2_rr_review;
mod v3_drill;
mod v3_protocol;
mod v3_review;
mod v3_roundrobin;

use askama::Template;
use axum::{
    Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{Html, IntoResponse, Json, Redirect},
    routing::get,
};
use lobby::{Lobby, TurnSubmission};
use protocol::{AgentToServer, ServerToAgent, SpectatorToServer};
use rand::{SeedableRng, rngs::StdRng};
use roundrobin::RoundRobin;
use serde::{Deserialize, Serialize};
use simulate_everything_engine::{
    agent::Agent,
    game::Game,
    mapgen::{self, MapConfig},
    replay::Replay,
    scoreboard::Scoreboard,
    v2,
};
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tokio::sync::{Mutex, broadcast};
use tower_http::services::ServeDir;
use tracing::{info, warn};
use v2_roundrobin::V2RoundRobin;
use v3_roundrobin::V3RoundRobin;

// ============================================================
// Shared state
// ============================================================

struct AppState {
    lobby: Arc<Lobby>,
    rr: Arc<RoundRobin>,
    v2_rr: Arc<V2RoundRobin>,
    v3_rr: Arc<V3RoundRobin>,
    v3_drill: Arc<v3_drill::V3Drill>,
    scoreboard: Arc<Mutex<Scoreboard>>,
    build_ver: String,
    /// Root `var/` directory for scanning behavior forensics bundles.
    var_dir: PathBuf,
    /// V3 RR review directory (for RR capture bundles).
    v3_rr_review_dir: PathBuf,
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
    let seed = params.seed.unwrap_or_else(rand::random);
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
        seed: params.seed.unwrap_or_else(rand::random),
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

#[derive(Template)]
#[template(path = "v3rr.html")]
struct V3RrTemplate {
    build_ver: String,
}

async fn v3_rr_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Html(
        V3RrTemplate {
            build_ver: state.build_ver.clone(),
        }
        .render()
        .unwrap(),
    )
}

#[derive(Template)]
#[template(path = "v3replay.html")]
struct V3ReplayTemplate {
    build_ver: String,
}

async fn v3_replay_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Html(
        V3ReplayTemplate {
            build_ver: state.build_ver.clone(),
        }
        .render()
        .unwrap(),
    )
}

// ============================================================
// V3 Review Gallery
// ============================================================

#[derive(Template)]
#[template(path = "v3reviews.html")]
struct V3ReviewsTemplate {
    build_ver: String,
}

async fn v3_reviews_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Html(
        V3ReviewsTemplate {
            build_ver: state.build_ver.clone(),
        }
        .render()
        .unwrap(),
    )
}

async fn api_v3_reviews_all(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let bundles =
        v3_review::list_all_bundles(&state.var_dir, &state.v3_rr_review_dir).await;
    Json(bundles)
}

async fn serve_review_file(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    // Reject path traversal attempts.
    if path.contains("..") {
        return (StatusCode::BAD_REQUEST, "invalid path").into_response();
    }

    // Try resolving against known bundle root directories.
    // Behavior bundles: var_dir/v3behavior_*/<scenario>/<file>
    // RR bundles: v3_rr_review_dir/<game>/<flag>/<file>
    let candidate = if path.starts_with("v3behavior_") {
        state.var_dir.join(&path)
    } else if path.starts_with("v3_reviews/") {
        // Strip the "v3_reviews/" prefix and resolve against rr review dir.
        let sub = path.strip_prefix("v3_reviews/").unwrap();
        state.v3_rr_review_dir.join(sub)
    } else {
        return (StatusCode::NOT_FOUND, "unknown bundle source").into_response();
    };

    // Canonicalize and verify containment.
    let canonical = match candidate.canonicalize() {
        Ok(p) => p,
        Err(_) => return (StatusCode::NOT_FOUND, "file not found").into_response(),
    };

    let var_canonical = state.var_dir.canonicalize().unwrap_or_default();
    let rr_canonical = state.v3_rr_review_dir.canonicalize().unwrap_or_default();

    if !canonical.starts_with(&var_canonical) && !canonical.starts_with(&rr_canonical) {
        return (StatusCode::FORBIDDEN, "access denied").into_response();
    }

    // Serve the file.
    match tokio::fs::read(&canonical).await {
        Ok(bytes) => {
            let content_type = match canonical.extension().and_then(|e| e.to_str()) {
                Some("html") => "text/html",
                Some("json") => "application/json",
                Some("png") => "image/png",
                Some("js") => "application/javascript",
                Some("css") => "text/css",
                _ => "application/octet-stream",
            };
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, content_type)],
                bytes,
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "file not found").into_response(),
    }
}

#[derive(Debug, Serialize)]
struct ReplayFileEntry {
    name: String,
    path: String,
    size_bytes: u64,
    modified_unix_ms: u64,
}

#[derive(Debug, Deserialize)]
struct ReplayFileQuery {
    path: String,
}

async fn api_v3_replay_files() -> Result<Json<Vec<ReplayFileEntry>>, StatusCode> {
    let mut entries = Vec::new();

    for root in replay_search_roots() {
        let Ok(read_dir) = fs::read_dir(&root) else {
            continue;
        };
        for item in read_dir.flatten() {
            let path = item.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(metadata) = item.metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let modified_unix_ms = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            entries.push(ReplayFileEntry {
                name: path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
                path: path.to_string_lossy().to_string(),
                size_bytes: metadata.len(),
                modified_unix_ms,
            });
        }
    }

    entries.sort_by(|a, b| {
        b.modified_unix_ms
            .cmp(&a.modified_unix_ms)
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(Json(entries))
}

async fn api_v3_replay_file(
    Query(query): Query<ReplayFileQuery>,
) -> Result<(StatusCode, String), StatusCode> {
    let requested = PathBuf::from(&query.path);
    let canonical = requested
        .canonicalize()
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !replay_search_roots()
        .iter()
        .any(|root| canonical.starts_with(root))
    {
        return Err(StatusCode::FORBIDDEN);
    }
    let body = fs::read_to_string(&canonical).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok((StatusCode::OK, body))
}

fn replay_search_roots() -> Vec<PathBuf> {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."));
    vec![PathBuf::from("/tmp"), repo_root.join("var")]
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
    state.v2_rr.broadcast_rr_status().await;
    Json(serde_json::json!({"ok": true}))
}

async fn api_v2_rr_pause(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.v2_rr.pause();
    state.v2_rr.broadcast_rr_status().await;
    Json(serde_json::json!({"ok": true, "paused": true}))
}

async fn api_v2_rr_resume(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.v2_rr.resume();
    state.v2_rr.broadcast_rr_status().await;
    Json(serde_json::json!({"ok": true, "paused": false}))
}

async fn api_v2_rr_reset(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.v2_rr.reset();
    state.v2_rr.broadcast_rr_status().await;
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
        Ok(result) => {
            state.v2_rr.broadcast_rr_status().await;
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "summary": result.summary,
                    "capturable_start_tick": result.capturable_start_tick,
                    "capturable_end_tick": result.capturable_end_tick,
                })),
            )
                .into_response()
        }
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
        Ok(result) => {
            state.v2_rr.broadcast_rr_status().await;
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "summary": result.summary,
                    "capturable_start_tick": result.capturable_start_tick,
                    "capturable_end_tick": result.capturable_end_tick,
                })),
            )
                .into_response()
        }
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
        Ok(result) => {
            state.v2_rr.broadcast_rr_status().await;
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "ok": true,
                    "summary": result.summary,
                    "capturable_start_tick": result.capturable_start_tick,
                    "capturable_end_tick": result.capturable_end_tick,
                })),
            )
                .into_response()
        }
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
// V3 Round-Robin
// ============================================================

#[derive(Deserialize)]
struct WsFormatQuery {
    format: Option<String>,
}

async fn ws_v3_rr(
    ws: WebSocketUpgrade,
    Query(query): Query<WsFormatQuery>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let rx = state.v3_rr.spectator_subscribe();
    let catchup = state.v3_rr.spectator_catchup().await;
    let use_json = query.format.as_deref() == Some("json");
    ws.on_upgrade(move |socket| handle_v3_spectator(socket, rx, catchup, use_json))
}

async fn handle_v3_spectator(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<v3_protocol::V3ServerToSpectator>,
    catchup: Vec<v3_protocol::V3ServerToSpectator>,
    use_json: bool,
) {
    for msg in catchup {
        if send_v3_msg(&mut socket, &msg, use_json).await.is_err() {
            return;
        }
    }

    loop {
        match rx.recv().await {
            Ok(msg) => {
                if send_v3_msg(&mut socket, &msg, use_json).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!("V3 spectator lagged, dropped {} messages", n);
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Send a V3 message as JSON (Text) or msgpack (Binary).
async fn send_v3_msg(
    socket: &mut WebSocket,
    msg: &v3_protocol::V3ServerToSpectator,
    use_json: bool,
) -> Result<(), axum::Error> {
    if use_json {
        let text = serde_json::to_string(msg).unwrap();
        socket.send(Message::Text(text.into())).await
    } else {
        let bytes = simulate_everything_protocol::encode(msg).unwrap();
        socket.send(Message::Binary(bytes.into())).await
    }
}

#[derive(Deserialize)]
struct V3RrConfigUpdate {
    tick_ms: Option<u64>,
    mode: Option<String>,
    autoplay: Option<bool>,
}

async fn api_v3_rr_config(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<V3RrConfigUpdate>,
) -> impl IntoResponse {
    if let Some(ms) = body.tick_ms {
        state.v3_rr.set_tick_ms(ms);
    }
    if let Some(ref mode_str) = body.mode {
        let mode = match mode_str.as_str() {
            "strategic" | "Strategic" => Some(v3_protocol::TimeMode::Strategic),
            "tactical" | "Tactical" => Some(v3_protocol::TimeMode::Tactical),
            "cinematic" | "Cinematic" => Some(v3_protocol::TimeMode::Cinematic),
            _ => None,
        };
        if let Some(m) = mode {
            state.v3_rr.set_mode(m).await;
        }
    }
    if let Some(ap) = body.autoplay {
        state.v3_rr.set_autoplay(ap);
    }
    let mode_val = body.mode.as_deref().and_then(|s| match s {
        "strategic" | "Strategic" => Some(v3_protocol::TimeMode::Strategic),
        "tactical" | "Tactical" => Some(v3_protocol::TimeMode::Tactical),
        "cinematic" | "Cinematic" => Some(v3_protocol::TimeMode::Cinematic),
        _ => None,
    });
    state
        .v3_rr
        .broadcast_config(body.tick_ms, mode_val, body.autoplay)
        .await;
    state.v3_rr.broadcast_rr_status().await;
    Json(serde_json::json!({"ok": true}))
}

async fn api_v3_rr_pause(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.v3_rr.pause();
    state.v3_rr.broadcast_rr_status().await;
    Json(serde_json::json!({"ok": true, "paused": true}))
}

async fn api_v3_rr_resume(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.v3_rr.resume();
    state.v3_rr.broadcast_rr_status().await;
    Json(serde_json::json!({"ok": true, "paused": false}))
}

async fn api_v3_rr_reset(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.v3_rr.reset();
    state.v3_rr.broadcast_rr_status().await;
    Json(serde_json::json!({"ok": true, "reset": true}))
}

async fn api_v3_rr_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let status = serde_json::to_value(state.v3_rr.spectator_catchup().await.into_iter().find_map(
        |m| {
            if let v3_protocol::V3ServerToSpectator::RrStatus(s) = m {
                Some(s)
            } else {
                None
            }
        },
    ))
    .unwrap_or(serde_json::json!({}));
    Json(status)
}

#[derive(Deserialize)]
struct V3RrFlagRequest {
    game_number: u64,
    tick: u64,
    #[serde(default)]
    annotation: String,
}

async fn api_v3_rr_flag(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<V3RrFlagRequest>,
) -> impl IntoResponse {
    match state
        .v3_rr
        .flag_tick(body.game_number, body.tick, body.annotation)
        .await
    {
        Ok(result) => {
            state.v3_rr.broadcast_rr_status().await;
            (StatusCode::OK, Json(serde_json::to_value(result).unwrap())).into_response()
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "ok": false, "error": err })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct V3CaptureRequest {
    game_number: u64,
}

async fn api_v3_rr_capture_start(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<V3CaptureRequest>,
) -> impl IntoResponse {
    match state.v3_rr.start_capture(body.game_number).await {
        Ok(result) => {
            state.v3_rr.broadcast_rr_status().await;
            (StatusCode::OK, Json(serde_json::to_value(result).unwrap())).into_response()
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "ok": false, "error": err })),
        )
            .into_response(),
    }
}

async fn api_v3_rr_capture_stop(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<V3CaptureRequest>,
) -> impl IntoResponse {
    match state.v3_rr.stop_capture(body.game_number).await {
        Ok(result) => {
            state.v3_rr.broadcast_rr_status().await;
            (StatusCode::OK, Json(serde_json::to_value(result).unwrap())).into_response()
        }
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "ok": false, "error": err })),
        )
            .into_response(),
    }
}

async fn api_v3_rr_reviews(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.v3_rr.list_reviews().await {
        Ok(reviews) => (StatusCode::OK, Json(serde_json::json!(reviews))).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn api_v3_rr_delete_review(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.v3_rr.delete_review(&id).await {
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
// V3 Drill Pad
// ============================================================

#[derive(Template)]
#[template(path = "v3drill.html")]
struct V3DrillTemplate {
    build_ver: String,
}

async fn v3_drill_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Html(
        V3DrillTemplate {
            build_ver: state.build_ver.clone(),
        }
        .render()
        .unwrap(),
    )
}

async fn api_v3_drill_exec(
    State(state): State<Arc<AppState>>,
    Json(req): Json<v3_drill::ExecRequest>,
) -> impl IntoResponse {
    let resp = state.v3_drill.exec(req).await;
    Json(resp)
}

async fn api_v3_drill_status(
    State(state): State<Arc<AppState>>,
    Query(view): Query<v3_drill::ViewParams>,
) -> impl IntoResponse {
    let resp = state.v3_drill.status(Some(view)).await;
    Json(resp)
}

async fn api_v3_drill_ascii(
    State(state): State<Arc<AppState>>,
    Query(view): Query<v3_drill::ViewParams>,
) -> impl IntoResponse {
    let resp = state.v3_drill.status(Some(view)).await;
    resp.ascii
}

async fn api_v3_drill_reset(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.v3_drill.reset().await;
    Json(serde_json::json!({ "ok": true }))
}

async fn api_v3_drill_zoo(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.v3_drill.zoo().await;
    Json(serde_json::json!({ "ok": true }))
}

async fn ws_v3_drill(
    ws: WebSocketUpgrade,
    Query(query): Query<WsFormatQuery>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let rx = state.v3_drill.spectator_subscribe();
    let catchup = state.v3_drill.spectator_catchup().await;
    let use_json = query.format.as_deref() == Some("json");
    ws.on_upgrade(move |socket| handle_v3_drill_spectator(socket, rx, catchup, use_json))
}

async fn handle_v3_drill_spectator(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<v3_protocol::V3ServerToSpectator>,
    catchup: Vec<v3_protocol::V3ServerToSpectator>,
    use_json: bool,
) {
    for msg in catchup {
        if send_v3_msg(&mut socket, &msg, use_json).await.is_err() {
            return;
        }
    }

    loop {
        match rx.recv().await {
            Ok(msg) => {
                if send_v3_msg(&mut socket, &msg, use_json).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!("V3 drill spectator lagged, dropped {} messages", n);
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
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
    let viewer_dir = std::env::var("SIMEV_VIEWER_DIR").unwrap_or_else(|_| {
        let manifest = env!("CARGO_MANIFEST_DIR");
        format!("{}/../../crates/viewer/dist", manifest)
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

    let v3_review_dir = std::env::var("SIMEV_V3_RR_REVIEW_DIR").unwrap_or_else(|_| {
        let manifest = env!("CARGO_MANIFEST_DIR");
        format!("{}/../../var/v3_reviews", manifest)
    });
    let v3_rr_review_dir: PathBuf = v3_review_dir.clone().into();
    let v3_rr = Arc::new(V3RoundRobin::new(tick_ms, v3_review_dir.into()));
    let v3_rr_loop = v3_rr.clone();
    tokio::spawn(async move {
        v3_rr_loop.run_loop().await;
    });

    // var/ directory for scanning behavior forensics bundles.
    let var_dir: PathBuf = {
        let manifest = env!("CARGO_MANIFEST_DIR");
        PathBuf::from(format!("{}/../../var", manifest))
    };

    let build_ver = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();
    let v3_drill = Arc::new(v3_drill::V3Drill::new());

    let state = Arc::new(AppState {
        lobby,
        rr,
        v2_rr,
        v3_rr,
        v3_drill,
        scoreboard,
        build_ver,
        var_dir,
        v3_rr_review_dir,
    });

    info!("Serving static files from: {}", static_dir);
    info!("Serving viewer files from: {}", viewer_dir);

    let app = Router::new()
        .route("/", get(|| async { Redirect::temporary("/v3/rr") }))
        // V3 RR routes
        .route("/v3/rr", get(v3_rr_page))
        .route("/rr", get(|| async { Redirect::temporary("/v3/rr") }))
        .route("/v1", get(|| async { Redirect::temporary("/v3/rr") }))
        .route("/v1/rr", get(|| async { Redirect::temporary("/v3/rr") }))
        .route("/v2", get(|| async { Redirect::temporary("/v3/rr") }))
        .route("/v2/rr", get(|| async { Redirect::temporary("/v3/rr") }))
        .route("/live", get(|| async { Redirect::temporary("/v3/rr") }))
        .route(
            "/scoreboard",
            get(|| async { Redirect::temporary("/v3/rr") }),
        )
        .route(
            "/v3/replay",
            get(|| async { Redirect::temporary("/v3/rr") }),
        )
        .route("/v3/drill", get(|| async { Redirect::temporary("/v3/rr") }))
        .route("/ws/v3/rr", get(ws_v3_rr))
        .route("/api/v3/rr/config", axum::routing::post(api_v3_rr_config))
        .route("/api/v3/rr/pause", axum::routing::post(api_v3_rr_pause))
        .route("/api/v3/rr/resume", axum::routing::post(api_v3_rr_resume))
        .route("/api/v3/rr/reset", axum::routing::post(api_v3_rr_reset))
        .route("/api/v3/rr/status", get(api_v3_rr_status))
        .route("/api/v3/rr/flags", axum::routing::post(api_v3_rr_flag))
        .route(
            "/api/v3/rr/capture/start",
            axum::routing::post(api_v3_rr_capture_start),
        )
        .route(
            "/api/v3/rr/capture/stop",
            axum::routing::post(api_v3_rr_capture_stop),
        )
        .route("/api/v3/rr/reviews", get(api_v3_rr_reviews))
        .route(
            "/api/v3/rr/reviews/{id}",
            axum::routing::delete(api_v3_rr_delete_review),
        )
        // V3 Review Gallery routes
        .route("/v3/reviews", get(v3_reviews_page))
        .route("/api/v3/reviews/all", get(api_v3_reviews_all))
        .route("/reviews/files/{*path}", get(serve_review_file))
        .nest_service("/viewer", ServeDir::new(&viewer_dir))
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
