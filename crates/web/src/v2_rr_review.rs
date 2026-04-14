use serde::{Deserialize, Serialize};
use simulate_everything_engine::v2::{
    TIMEOUT_TICKS,
    gamelog::{AgentPollRecord, EconomySample, GameEvent, GameLog, UnitPositionSample},
    hex::Axial,
    mapgen::MapConfig,
    replay::{self, Frame, Replay, StaticCellSnapshot},
    sim,
    state::GameState,
};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{fs, sync::Mutex};

pub const REVIEW_PRE_TICKS: u64 = 5;
pub const REVIEW_POST_TICKS: u64 = 10;
pub const REVIEW_BUFFER_TICKS: usize = 600;
const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlapAnomaly {
    pub tick: u64,
    pub q: i32,
    pub r: i32,
    pub owners: Vec<u8>,
    pub unit_ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewLogWindow {
    pub events: Vec<GameEvent>,
    pub agent_polls: Vec<AgentPollRecord>,
    pub economy_samples: Vec<EconomySample>,
    pub unit_positions: Vec<UnitPositionSample>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReviewKind {
    Point,
    Segment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewBundleSummary {
    pub id: String,
    pub created_at: u64,
    pub kind: ReviewKind,
    pub game_number: u64,
    pub seed: u64,
    pub agent_names: Vec<String>,
    pub start_tick: u64,
    pub stop_tick: Option<u64>,
    pub flagged_ticks: Vec<u64>,
    pub range_start: u64,
    pub range_end: u64,
    pub complete: bool,
    pub saved: bool,
    pub anomaly_count: usize,
    pub event_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewBundle {
    #[serde(flatten)]
    pub summary: ReviewBundleSummary,
    pub replay: Replay,
    pub anomalies: Vec<OverlapAnomaly>,
    pub log: ReviewLogWindow,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ReviewManifest {
    reviews: Vec<ReviewBundleSummary>,
}

#[derive(Debug, Clone)]
struct PendingCapture {
    id: String,
    created_at: u64,
    flagged_ticks: Vec<u64>,
    start_tick: u64,
    stop_tick: u64,
    range_start: u64,
    range_end: u64,
}

impl PendingCapture {
    fn summary(&self, game_number: u64, seed: u64, agent_names: &[String]) -> ReviewBundleSummary {
        ReviewBundleSummary {
            id: self.id.clone(),
            created_at: self.created_at,
            kind: ReviewKind::Point,
            game_number,
            seed,
            agent_names: agent_names.to_vec(),
            start_tick: self.start_tick,
            stop_tick: Some(self.stop_tick),
            flagged_ticks: self.flagged_ticks.clone(),
            range_start: self.range_start,
            range_end: self.range_end,
            complete: false,
            saved: false,
            anomaly_count: 0,
            event_count: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct ActiveSegmentCapture {
    id: String,
    created_at: u64,
    start_tick: u64,
    frames: Vec<Frame>,
    anomalies: Vec<OverlapAnomaly>,
}

impl ActiveSegmentCapture {
    fn summary(
        &self,
        game_number: u64,
        seed: u64,
        agent_names: &[String],
        current_tick: u64,
    ) -> ReviewBundleSummary {
        ReviewBundleSummary {
            id: self.id.clone(),
            created_at: self.created_at,
            kind: ReviewKind::Segment,
            game_number,
            seed,
            agent_names: agent_names.to_vec(),
            start_tick: self.start_tick,
            stop_tick: None,
            flagged_ticks: Vec::new(),
            range_start: self.start_tick,
            range_end: current_tick.max(self.start_tick),
            complete: false,
            saved: false,
            anomaly_count: 0,
            event_count: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct ReviewReplayMeta {
    width: usize,
    height: usize,
    terrain: Vec<f32>,
    material_map: Vec<f32>,
    static_cells: Vec<StaticCellSnapshot>,
    num_players: usize,
    agent_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FlagResponse {
    pub summary: ReviewBundleSummary,
    pub capturable_start_tick: u64,
    pub capturable_end_tick: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewStatus {
    pub game_number: Option<u64>,
    pub current_tick: Option<u64>,
    pub capturable_start_tick: Option<u64>,
    pub capturable_end_tick: Option<u64>,
    pub pending_capture_count: usize,
    pub active_capture: Option<ReviewBundleSummary>,
    pub review_dir: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewListResponse {
    pub pending: Vec<ReviewBundleSummary>,
    pub saved: Vec<ReviewBundleSummary>,
}

#[derive(Clone)]
pub struct ReviewStore {
    root: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl ReviewStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    async fn ensure_root(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.root).await
    }

    fn manifest_path(&self) -> PathBuf {
        self.root.join(MANIFEST_FILE)
    }

    fn bundle_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.json"))
    }

    async fn read_manifest(&self) -> std::io::Result<ReviewManifest> {
        let path = self.manifest_path();
        match fs::read(&path).await {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(ReviewManifest::default()),
            Err(err) => Err(err),
        }
    }

    async fn write_manifest(&self, manifest: &ReviewManifest) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(manifest)?;
        fs::write(self.manifest_path(), bytes).await
    }

    pub async fn save_bundle(&self, bundle: ReviewBundle) -> std::io::Result<()> {
        let _guard = self.lock.lock().await;
        self.ensure_root().await?;
        let mut manifest = self.read_manifest().await?;
        manifest
            .reviews
            .retain(|entry| entry.id != bundle.summary.id);
        manifest.reviews.push(bundle.summary.clone());
        manifest.reviews.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        let bytes = serde_json::to_vec_pretty(&bundle)?;
        fs::write(self.bundle_path(&bundle.summary.id), bytes).await?;
        self.write_manifest(&manifest).await
    }

    pub async fn list_summaries(&self) -> std::io::Result<Vec<ReviewBundleSummary>> {
        let _guard = self.lock.lock().await;
        self.ensure_root().await?;
        Ok(self.read_manifest().await?.reviews)
    }

    pub async fn load_bundle(&self, id: &str) -> std::io::Result<Option<ReviewBundle>> {
        let _guard = self.lock.lock().await;
        self.ensure_root().await?;
        match fs::read(self.bundle_path(id)).await {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    pub async fn delete_bundle(&self, id: &str) -> std::io::Result<bool> {
        let _guard = self.lock.lock().await;
        self.ensure_root().await?;
        let mut manifest = self.read_manifest().await?;
        let before = manifest.reviews.len();
        manifest.reviews.retain(|entry| entry.id != id);
        let removed = manifest.reviews.len() != before;
        if removed {
            let _ = fs::remove_file(self.bundle_path(id)).await;
            self.write_manifest(&manifest).await?;
        }
        Ok(removed)
    }
}

pub struct ReviewRecorder {
    store: ReviewStore,
    game_number: Option<u64>,
    seed: Option<u64>,
    current_tick: Option<u64>,
    next_capture_seq: u64,
    meta: Option<ReviewReplayMeta>,
    frames: VecDeque<Frame>,
    anomalies: Vec<OverlapAnomaly>,
    log: ReviewLogWindow,
    pending_points: Vec<PendingCapture>,
    active_segment: Option<ActiveSegmentCapture>,
}

impl ReviewRecorder {
    pub fn new(store: ReviewStore) -> Self {
        Self {
            store,
            game_number: None,
            seed: None,
            current_tick: None,
            next_capture_seq: 0,
            meta: None,
            frames: VecDeque::new(),
            anomalies: Vec::new(),
            log: ReviewLogWindow {
                events: Vec::new(),
                agent_polls: Vec::new(),
                economy_samples: Vec::new(),
                unit_positions: Vec::new(),
            },
            pending_points: Vec::new(),
            active_segment: None,
        }
    }

    pub fn start_game(
        &mut self,
        game_number: u64,
        config: &MapConfig,
        state: &GameState,
        agent_names: &[String],
    ) {
        self.game_number = Some(game_number);
        self.seed = Some(config.seed);
        self.current_tick = Some(state.tick);
        self.next_capture_seq = 0;
        self.frames.clear();
        self.anomalies.clear();
        self.log = ReviewLogWindow {
            events: Vec::new(),
            agent_polls: Vec::new(),
            economy_samples: Vec::new(),
            unit_positions: Vec::new(),
        };
        self.pending_points.clear();
        self.active_segment = None;
        self.meta = Some(ReviewReplayMeta {
            width: state.width,
            height: state.height,
            terrain: state.grid.iter().map(|c| c.terrain_value).collect(),
            material_map: state.grid.iter().map(|c| c.material_value).collect(),
            static_cells: replay::snapshot_static_cells(state),
            num_players: state.players.len(),
            agent_names: agent_names.to_vec(),
        });
        self.record_tick(state);
    }

    pub fn record_tick(&mut self, state: &GameState) {
        self.current_tick = Some(state.tick);
        let frame = replay::capture_frame(state);
        self.frames.push_back(frame.clone());
        while self.frames.len() > REVIEW_BUFFER_TICKS {
            self.frames.pop_front();
        }
        self.append_log_deltas(state.game_log.as_ref());
        let tick_anomalies = detect_overlap_anomalies(state);
        self.anomalies.extend(tick_anomalies.iter().cloned());
        if let Some(active) = &mut self.active_segment
            && state.tick >= active.start_tick
        {
            active.frames.push(frame);
            active.anomalies.extend(tick_anomalies);
        }
    }

    pub fn status(&self) -> ReviewStatus {
        let current_tick = self.current_tick;
        ReviewStatus {
            game_number: self.game_number,
            current_tick,
            capturable_start_tick: self.frames.front().map(|f| f.tick),
            capturable_end_tick: self.frames.back().map(|f| f.tick),
            pending_capture_count: self.pending_points.len()
                + usize::from(self.active_segment.is_some()),
            active_capture: self.active_segment_summary(),
            review_dir: self.store.root().display().to_string(),
        }
    }

    fn active_segment_summary(&self) -> Option<ReviewBundleSummary> {
        let game_number = self.game_number?;
        let current_tick = self.current_tick?;
        let seed = self.seed.unwrap_or(0);
        let agent_names = self
            .meta
            .as_ref()
            .map(|meta| meta.agent_names.as_slice())
            .unwrap_or(&[]);
        self.active_segment
            .as_ref()
            .map(|capture| capture.summary(game_number, seed, agent_names, current_tick))
    }

    pub fn pending_summaries(&self) -> Vec<ReviewBundleSummary> {
        let Some(game_number) = self.game_number else {
            return Vec::new();
        };
        let Some(current_tick) = self.current_tick else {
            return Vec::new();
        };
        let seed = self.seed.unwrap_or(0);
        let agent_names = self
            .meta
            .as_ref()
            .map(|meta| meta.agent_names.as_slice())
            .unwrap_or(&[]);
        let mut pending: Vec<_> = self
            .pending_points
            .iter()
            .map(|capture| capture.summary(game_number, seed, agent_names))
            .collect();
        if let Some(active) = &self.active_segment {
            pending.push(active.summary(game_number, seed, agent_names, current_tick));
        }
        pending.sort_by_key(|summary| (summary.range_start, summary.created_at));
        pending
    }

    fn validate_game_and_tick(
        &self,
        game_number: u64,
        tick: u64,
    ) -> Result<(u64, u64, u64), String> {
        let current_game = self
            .game_number
            .ok_or_else(|| String::from("RR game not initialized"))?;
        if current_game != game_number {
            return Err(format!(
                "tick belongs to stale game {game_number}; current game is {current_game}"
            ));
        }
        let start = self
            .frames
            .front()
            .map(|frame| frame.tick)
            .ok_or_else(|| String::from("RR review buffer is empty"))?;
        let end = self
            .frames
            .back()
            .map(|frame| frame.tick)
            .ok_or_else(|| String::from("RR review buffer is empty"))?;
        if tick < start || tick > end {
            return Err(format!(
                "tick {tick} is outside capturable range {start}..={end}"
            ));
        }
        Ok((current_game, start, end))
    }

    pub fn flag_tick(&mut self, game_number: u64, tick: u64) -> Result<FlagResponse, String> {
        let (current_game, start, end) = self.validate_game_and_tick(game_number, tick)?;

        let requested_start = tick.saturating_sub(REVIEW_PRE_TICKS);
        let requested_end = tick.saturating_add(REVIEW_POST_TICKS);
        let created_at = unix_ts();
        let id = format!(
            "g{}-t{}-{}-{}",
            current_game, requested_start, requested_end, self.next_capture_seq
        );
        self.next_capture_seq += 1;
        let mut merged = PendingCapture {
            id,
            created_at,
            flagged_ticks: vec![tick],
            start_tick: tick,
            stop_tick: tick,
            range_start: requested_start,
            range_end: requested_end,
        };

        let mut retained = Vec::with_capacity(self.pending_points.len() + 1);
        for existing in self.pending_points.drain(..) {
            if windows_touch_or_overlap(
                merged.range_start,
                merged.range_end,
                existing.range_start,
                existing.range_end,
            ) {
                merged.created_at = merged.created_at.min(existing.created_at);
                merged.start_tick = merged.start_tick.min(existing.start_tick);
                merged.stop_tick = merged.stop_tick.max(existing.stop_tick);
                merged.range_start = merged.range_start.min(existing.range_start);
                merged.range_end = merged.range_end.max(existing.range_end);
                merged.flagged_ticks.extend(existing.flagged_ticks);
                merged.flagged_ticks.sort_unstable();
                merged.flagged_ticks.dedup();
                if existing.id < merged.id {
                    merged.id = existing.id;
                }
            } else {
                retained.push(existing);
            }
        }
        retained.push(merged.clone());
        retained.sort_by_key(|capture| capture.range_start);
        self.pending_points = retained;

        let seed = self.seed.unwrap_or(0);
        let agent_names = self
            .meta
            .as_ref()
            .map(|meta| meta.agent_names.as_slice())
            .unwrap_or(&[]);
        Ok(FlagResponse {
            summary: merged.summary(current_game, seed, agent_names),
            capturable_start_tick: start,
            capturable_end_tick: end,
        })
    }

    pub fn start_capture(&mut self, game_number: u64, tick: u64) -> Result<FlagResponse, String> {
        let (current_game, capturable_start_tick, capturable_end_tick) =
            self.validate_game_and_tick(game_number, tick)?;
        if self.active_segment.is_some() {
            return Err(String::from("a segment capture is already active"));
        }
        let created_at = unix_ts();
        let id = format!("g{}-seg-{}-{}", current_game, tick, self.next_capture_seq);
        self.next_capture_seq += 1;
        let frames = self
            .frames
            .iter()
            .filter(|frame| frame.tick >= tick)
            .cloned()
            .collect();
        let anomalies = self
            .anomalies
            .iter()
            .filter(|anomaly| anomaly.tick >= tick)
            .cloned()
            .collect();
        self.active_segment = Some(ActiveSegmentCapture {
            id,
            created_at,
            start_tick: tick,
            frames,
            anomalies,
        });
        Ok(FlagResponse {
            summary: self
                .active_segment_summary()
                .expect("active segment summary available"),
            capturable_start_tick,
            capturable_end_tick,
        })
    }

    pub fn stop_capture(&mut self, game_number: u64, tick: u64) -> Result<ReviewBundle, String> {
        let (_current_game, _capturable_start_tick, _capturable_end_tick) =
            self.validate_game_and_tick(game_number, tick)?;
        let active = self
            .active_segment
            .as_ref()
            .ok_or_else(|| String::from("no active segment capture"))?;
        if tick < active.start_tick {
            return Err(format!(
                "stop tick {tick} is before segment start tick {}",
                active.start_tick
            ));
        }
        let active = self.active_segment.take().expect("active segment present");
        Ok(self.build_segment_bundle(active, tick, true, None))
    }

    pub fn collect_ready_bundles(&mut self, state: &GameState) -> Vec<ReviewBundle> {
        self.collect_point_bundles_matching(state, |pending, current_tick| {
            pending.range_end <= current_tick
        })
    }

    pub fn finalize_all(&mut self, state: &GameState) -> Vec<ReviewBundle> {
        let mut bundles =
            self.collect_point_bundles_matching(state, |_pending, _current_tick| true);
        if let Some(active) = self.active_segment.take() {
            let final_tick = self
                .current_tick
                .unwrap_or(state.tick)
                .max(active.start_tick);
            bundles.push(self.build_segment_bundle(active, final_tick, false, Some(state)));
        }
        bundles
    }

    fn append_log_deltas(&mut self, log: Option<&GameLog>) {
        let Some(log) = log else {
            return;
        };
        self.log
            .events
            .extend(log.events[self.log.events.len()..].iter().cloned());
        self.log.agent_polls.extend(
            log.agent_polls[self.log.agent_polls.len()..]
                .iter()
                .cloned(),
        );
        self.log.economy_samples.extend(
            log.economy_samples[self.log.economy_samples.len()..]
                .iter()
                .cloned(),
        );
        self.log.unit_positions.extend(
            log.unit_positions[self.log.unit_positions.len()..]
                .iter()
                .cloned(),
        );
    }

    fn collect_point_bundles_matching<F>(
        &mut self,
        state: &GameState,
        predicate: F,
    ) -> Vec<ReviewBundle>
    where
        F: Fn(&PendingCapture, u64) -> bool,
    {
        let current_tick = self.current_tick.unwrap_or(state.tick);
        let drained: Vec<_> = self.pending_points.drain(..).collect();
        let mut ready = Vec::new();
        let mut retained = Vec::new();

        for pending in drained {
            if predicate(&pending, current_tick) {
                ready.push(self.build_bundle(state, &pending, current_tick >= pending.range_end));
            } else {
                retained.push(pending);
            }
        }
        self.pending_points = retained;
        ready
    }

    fn build_bundle(
        &self,
        state: &GameState,
        pending: &PendingCapture,
        complete: bool,
    ) -> ReviewBundle {
        let meta = self.meta.as_ref().expect("review meta initialized");
        let range_end = pending
            .range_end
            .min(self.current_tick.unwrap_or(state.tick));
        let frames: Vec<Frame> = self
            .frames
            .iter()
            .filter(|frame| frame.tick >= pending.range_start && frame.tick <= range_end)
            .cloned()
            .collect();
        let anomalies: Vec<OverlapAnomaly> = self
            .anomalies
            .iter()
            .filter(|anomaly| anomaly.tick >= pending.range_start && anomaly.tick <= range_end)
            .cloned()
            .collect();
        let log = slice_log_window(&self.log, pending.range_start, range_end);
        let event_count = log.events.len();
        let summary = ReviewBundleSummary {
            id: pending.id.clone(),
            created_at: pending.created_at,
            kind: ReviewKind::Point,
            game_number: self.game_number.unwrap_or(0),
            seed: self.seed.unwrap_or(0),
            agent_names: meta.agent_names.clone(),
            start_tick: pending.start_tick,
            stop_tick: Some(pending.stop_tick),
            flagged_ticks: pending.flagged_ticks.clone(),
            range_start: pending.range_start,
            range_end,
            complete,
            saved: true,
            anomaly_count: anomalies.len(),
            event_count,
        };
        let replay = Replay {
            width: meta.width,
            height: meta.height,
            terrain: meta.terrain.clone(),
            material_map: meta.material_map.clone(),
            static_cells: meta.static_cells.clone(),
            num_players: meta.num_players,
            agent_names: meta.agent_names.clone(),
            frames,
            winner: if complete {
                sim::winner_at_limit(state, sim::timeout_limit(TIMEOUT_TICKS))
            } else {
                None
            },
            timed_out: complete && sim::reached_timeout(state, sim::timeout_limit(TIMEOUT_TICKS)),
        };
        ReviewBundle {
            summary,
            replay,
            anomalies,
            log,
        }
    }

    fn build_segment_bundle(
        &self,
        active: ActiveSegmentCapture,
        stop_tick: u64,
        complete: bool,
        state: Option<&GameState>,
    ) -> ReviewBundle {
        let meta = self.meta.as_ref().expect("review meta initialized");
        let range_end = stop_tick.max(active.start_tick);
        let frames: Vec<Frame> = active
            .frames
            .into_iter()
            .filter(|frame| frame.tick >= active.start_tick && frame.tick <= range_end)
            .collect();
        let anomalies: Vec<OverlapAnomaly> = active
            .anomalies
            .into_iter()
            .filter(|anomaly| anomaly.tick >= active.start_tick && anomaly.tick <= range_end)
            .collect();
        let log = slice_log_window(&self.log, active.start_tick, range_end);
        let event_count = log.events.len();
        let summary = ReviewBundleSummary {
            id: active.id,
            created_at: active.created_at,
            kind: ReviewKind::Segment,
            game_number: self.game_number.unwrap_or(0),
            seed: self.seed.unwrap_or(0),
            agent_names: meta.agent_names.clone(),
            start_tick: active.start_tick,
            stop_tick: Some(range_end),
            flagged_ticks: Vec::new(),
            range_start: active.start_tick,
            range_end,
            complete,
            saved: true,
            anomaly_count: anomalies.len(),
            event_count,
        };
        let replay = Replay {
            width: meta.width,
            height: meta.height,
            terrain: meta.terrain.clone(),
            material_map: meta.material_map.clone(),
            static_cells: meta.static_cells.clone(),
            num_players: meta.num_players,
            agent_names: meta.agent_names.clone(),
            frames,
            winner: if complete {
                state.and_then(|state| {
                    sim::winner_at_limit(state, sim::timeout_limit(TIMEOUT_TICKS))
                })
            } else {
                None
            },
            timed_out: complete
                && state
                    .map(|state| sim::reached_timeout(state, sim::timeout_limit(TIMEOUT_TICKS)))
                    .unwrap_or(false),
        };
        ReviewBundle {
            summary,
            replay,
            anomalies,
            log,
        }
    }
}

fn slice_log_window(log: &ReviewLogWindow, start_tick: u64, end_tick: u64) -> ReviewLogWindow {
    ReviewLogWindow {
        events: log
            .events
            .iter()
            .filter(|event| event_tick(event) >= start_tick && event_tick(event) <= end_tick)
            .cloned()
            .collect(),
        agent_polls: log
            .agent_polls
            .iter()
            .filter(|poll| poll.tick >= start_tick && poll.tick <= end_tick)
            .cloned()
            .collect(),
        economy_samples: log
            .economy_samples
            .iter()
            .filter(|sample| sample.tick >= start_tick && sample.tick <= end_tick)
            .cloned()
            .collect(),
        unit_positions: log
            .unit_positions
            .iter()
            .filter(|sample| sample.tick >= start_tick && sample.tick <= end_tick)
            .cloned()
            .collect(),
    }
}

fn event_tick(event: &GameEvent) -> u64 {
    match event {
        GameEvent::UnitProduced { tick, .. }
        | GameEvent::UnitKilled { tick, .. }
        | GameEvent::EngagementCreated { tick, .. }
        | GameEvent::EngagementEnded { tick, .. }
        | GameEvent::SettlementFounded { tick, .. }
        | GameEvent::PlayerEliminated { tick, .. } => *tick,
    }
}

fn detect_overlap_anomalies(state: &GameState) -> Vec<OverlapAnomaly> {
    let mut by_hex: HashMap<Axial, Vec<(u8, u32)>> = HashMap::new();
    for unit in state.units.values() {
        by_hex
            .entry(unit.pos)
            .or_default()
            .push((unit.owner, unit.public_id));
    }
    let mut anomalies = Vec::new();
    for (pos, units) in by_hex {
        let mut owners: Vec<u8> = units.iter().map(|(owner, _)| *owner).collect();
        owners.sort_unstable();
        owners.dedup();
        if owners.len() < 2 {
            continue;
        }
        let mut unit_ids: Vec<u32> = units.iter().map(|(_, unit_id)| *unit_id).collect();
        unit_ids.sort_unstable();
        anomalies.push(OverlapAnomaly {
            tick: state.tick,
            q: pos.q,
            r: pos.r,
            owners,
            unit_ids,
        });
    }
    anomalies.sort_by_key(|anomaly| (anomaly.tick, anomaly.q, anomaly.r));
    anomalies
}

fn windows_touch_or_overlap(a_start: u64, a_end: u64, b_start: u64, b_end: u64) -> bool {
    a_start <= b_end.saturating_add(1) && b_start <= a_end.saturating_add(1)
}

fn unix_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use simulate_everything_engine::v2::mapgen;

    fn test_state(seed: u64) -> GameState {
        let mut state = mapgen::generate(&MapConfig {
            width: 10,
            height: 10,
            num_players: 2,
            seed,
        });
        state.game_log = Some(GameLog::new());
        state
    }

    #[test]
    fn windows_merge_when_overlapping() {
        let store = ReviewStore::new(PathBuf::from("/tmp/unused"));
        let mut recorder = ReviewRecorder::new(store);
        let state = test_state(1);
        let agent_names = vec![String::from("a"), String::from("b")];
        let config = MapConfig {
            width: 10,
            height: 10,
            num_players: 2,
            seed: 1,
        };
        recorder.start_game(1, &config, &state, &agent_names);
        for tick in 1..=20 {
            let mut state = test_state(1);
            state.tick = tick;
            recorder.record_tick(&state);
        }

        recorder.flag_tick(1, 10).unwrap();
        recorder.flag_tick(1, 14).unwrap();

        assert_eq!(recorder.pending_points.len(), 1);
        assert_eq!(recorder.pending_points[0].range_start, 5);
        assert_eq!(recorder.pending_points[0].range_end, 24);
    }

    #[test]
    fn flag_rejects_out_of_range_tick() {
        let store = ReviewStore::new(PathBuf::from("/tmp/unused"));
        let mut recorder = ReviewRecorder::new(store);
        let state = test_state(2);
        let agent_names = vec![String::from("a"), String::from("b")];
        let config = MapConfig {
            width: 10,
            height: 10,
            num_players: 2,
            seed: 2,
        };
        recorder.start_game(1, &config, &state, &agent_names);
        assert!(recorder.flag_tick(1, 999).is_err());
    }

    #[test]
    fn segment_capture_requires_active_start() {
        let store = ReviewStore::new(PathBuf::from("/tmp/unused"));
        let mut recorder = ReviewRecorder::new(store);
        let state = test_state(3);
        let agent_names = vec![String::from("a"), String::from("b")];
        let config = MapConfig {
            width: 10,
            height: 10,
            num_players: 2,
            seed: 3,
        };
        recorder.start_game(1, &config, &state, &agent_names);
        assert!(recorder.stop_capture(1, 0).is_err());
    }

    #[test]
    fn segment_capture_records_exact_window() {
        let store = ReviewStore::new(PathBuf::from("/tmp/unused"));
        let mut recorder = ReviewRecorder::new(store);
        let state = test_state(4);
        let agent_names = vec![String::from("a"), String::from("b")];
        let config = MapConfig {
            width: 10,
            height: 10,
            num_players: 2,
            seed: 4,
        };
        recorder.start_game(1, &config, &state, &agent_names);
        for tick in 1..=20 {
            let mut state = test_state(4);
            state.tick = tick;
            recorder.record_tick(&state);
        }

        recorder.start_capture(1, 6).unwrap();
        let bundle = recorder.stop_capture(1, 12).unwrap();

        assert_eq!(bundle.summary.kind, ReviewKind::Segment);
        assert_eq!(bundle.summary.start_tick, 6);
        assert_eq!(bundle.summary.stop_tick, Some(12));
        assert_eq!(bundle.summary.range_start, 6);
        assert_eq!(bundle.summary.range_end, 12);
        assert!(bundle.summary.complete);
        assert!(
            bundle
                .replay
                .frames
                .iter()
                .all(|frame| frame.tick >= 6 && frame.tick <= 12)
        );
    }

    #[test]
    fn finalize_saves_incomplete_segment() {
        let store = ReviewStore::new(PathBuf::from("/tmp/unused"));
        let mut recorder = ReviewRecorder::new(store);
        let state = test_state(5);
        let agent_names = vec![String::from("a"), String::from("b")];
        let config = MapConfig {
            width: 10,
            height: 10,
            num_players: 2,
            seed: 5,
        };
        recorder.start_game(1, &config, &state, &agent_names);
        for tick in 1..=15 {
            let mut state = test_state(5);
            state.tick = tick;
            recorder.record_tick(&state);
        }

        recorder.start_capture(1, 4).unwrap();
        let mut state = test_state(5);
        state.tick = 15;
        let bundles = recorder.finalize_all(&state);

        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].summary.kind, ReviewKind::Segment);
        assert_eq!(bundles[0].summary.start_tick, 4);
        assert_eq!(bundles[0].summary.stop_tick, Some(15));
        assert!(!bundles[0].summary.complete);
    }
}
