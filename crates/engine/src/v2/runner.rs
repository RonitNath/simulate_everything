use super::AGENT_POLL_INTERVAL;
use super::agent::Agent;
use super::directive;
use super::observation;
use super::sim;
use super::state::GameState;

pub(crate) fn advance_game_tick(state: &mut GameState, agents: &mut [Box<dyn Agent>]) {
    if state.tick % AGENT_POLL_INTERVAL as u64 == 0 {
        let mut session = observation::ObservationSession::new(state.players.len(), state.width * state.height);
        for (player_id, agent) in agents.iter_mut().enumerate() {
            let pid = player_id as u8;
            if !state.players.iter().any(|p| p.id == pid && p.alive) {
                continue;
            }
            let init = observation::initial_observation(state, pid);
            agent.reset();
            agent.init(&init);
            let delta = observation::observe_delta(state, pid, &mut session);
            let directives = agent.act(&delta);
            tracing::trace!(
                tick = state.tick,
                player = pid,
                directives = directives.len(),
                "agent polled"
            );
            directive::apply_directives(state, pid, &directives);
        }
        state.clear_dirty_hexes();
    }

    sim::tick(state);
}

pub fn run_loop<F>(
    state: &mut GameState,
    agents: &mut [Box<dyn Agent>],
    tick_limit: u64,
    mut after_tick: F,
) where
    F: FnMut(&GameState),
{
    let mut session = observation::ObservationSession::new(state.players.len(), state.width * state.height);
    for (player_id, agent) in agents.iter_mut().enumerate() {
        let pid = player_id as u8;
        let init = observation::initial_observation(state, pid);
        agent.reset();
        agent.init(&init);
    }
    while state.tick < tick_limit && !sim::is_over(state) {
        if state.tick % AGENT_POLL_INTERVAL as u64 == 0 {
            for (player_id, agent) in agents.iter_mut().enumerate() {
                let pid = player_id as u8;
                if !state.players.iter().any(|p| p.id == pid && p.alive) {
                    continue;
                }
                let delta = observation::observe_delta(state, pid, &mut session);
                let directives = agent.act(&delta);
                tracing::trace!(
                    tick = state.tick,
                    player = pid,
                    directives = directives.len(),
                    "agent polled"
                );

                // Record agent poll before applying directives
                if let Some(log) = &mut state.game_log {
                    let mut move_count = 0u16;
                    let mut engage_count = 0u16;
                    let mut produce_count = 0u16;
                    let mut other_count = 0u16;
                    for d in &directives {
                        match d {
                            directive::Directive::Move { .. } => move_count += 1,
                            directive::Directive::Engage { .. } => engage_count += 1,
                            directive::Directive::Produce { .. } => produce_count += 1,
                            _ => other_count += 1,
                        }
                    }
                    log.record_poll(super::gamelog::AgentPollRecord {
                        tick: state.tick,
                        player: pid,
                        move_count,
                        engage_count,
                        produce_count,
                        other_count,
                        mode: agent.mode().map(String::from),
                    });
                }

                directive::apply_directives(state, pid, &directives);
            }
            state.clear_dirty_hexes();
        }

        sim::tick(state);

        // Economy sampling every 50 ticks
        if state.game_log.is_some() && state.tick % 50 == 0 {
            let player_ids: Vec<u8> = state
                .players
                .iter()
                .filter(|p| p.alive)
                .map(|p| p.id)
                .collect();
            for pid in player_ids {
                let sample = super::gamelog::GameLog::sample_economy(state, pid);
                if let Some(log) = &mut state.game_log {
                    log.record_economy(sample);
                }
            }
        }

        // Detect new settlements
        if state.game_log.is_some() {
            let settlements = super::gamelog::collect_settlements(state);
            let tick = state.tick;
            if let Some(log) = &mut state.game_log {
                log.detect_new_settlements(tick, &settlements);
            }
        }

        after_tick(state);
    }
}

/// Run a complete game with the given agents, polling each every AGENT_POLL_INTERVAL ticks.
/// Agents are indexed by player ID: agents[0] controls player 0, etc.
/// Returns the winner's player ID, or None for a draw / simultaneous elimination.
pub fn run_game(
    state: &mut GameState,
    agents: &mut [Box<dyn Agent>],
    max_ticks: u64,
) -> Option<u8> {
    let tick_limit = sim::timeout_limit(max_ticks);
    let agent_names: Vec<String> = agents.iter().map(|a| a.name().to_string()).collect();
    tracing::info!(
        width = state.width,
        height = state.height,
        players = state.players.len(),
        max_ticks = tick_limit,
        ?agent_names,
        "game starting"
    );

    run_loop(state, agents, tick_limit, |state| {
        if state.tick % 50 == 0 {
            for p in &state.players {
                if !p.alive {
                    continue;
                }
                let units: Vec<_> = state.units.values().filter(|u| u.owner == p.id).collect();
                let engaged = units.iter().filter(|u| !u.engagements.is_empty()).count();
                let moving = units
                    .iter()
                    .filter(|u| u.destination.is_some() && u.engagements.is_empty())
                    .count();
                let idle = units.len() - engaged - moving;
                let food = (p.food * 10.0).round() / 10.0;
                let material = (p.material * 10.0).round() / 10.0;
                tracing::debug!(
                    tick = state.tick,
                    player = p.id,
                    units = units.len(),
                    engaged,
                    moving,
                    idle,
                    food,
                    material,
                    "player status"
                );
            }
        }
    });

    let winner = sim::winner_at_limit(state, tick_limit);
    tracing::info!(
        tick = state.tick,
        timed_out = sim::reached_timeout(state, tick_limit),
        winner = ?winner,
        "game ended"
    );
    winner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::agent::SpreadAgent;
    use crate::v2::mapgen::{MapConfig, generate};

    #[test]
    fn run_game_completes() {
        let mut state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 42,
        });
        let mut agents: Vec<Box<dyn Agent>> =
            vec![Box::new(SpreadAgent::new()), Box::new(SpreadAgent::new())];
        run_game(&mut state, &mut agents, 5000);
        assert!(state.tick > 0, "game should advance");
        assert!(
            state.players.iter().any(|p| p.alive),
            "at least one player should remain"
        );
    }

    #[test]
    fn run_game_returns_winner() {
        let mut state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 100,
        });
        let mut agents: Vec<Box<dyn Agent>> =
            vec![Box::new(SpreadAgent::new()), Box::new(SpreadAgent::new())];
        let _result = run_game(&mut state, &mut agents, 5000);
    }

    #[test]
    fn run_game_no_panic_3_players() {
        let mut state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 3,
            seed: 77,
        });
        let mut agents: Vec<Box<dyn Agent>> = vec![
            Box::new(SpreadAgent::new()),
            Box::new(SpreadAgent::new()),
            Box::new(SpreadAgent::new()),
        ];
        let _result = run_game(&mut state, &mut agents, 3000);
    }

    #[test]
    fn run_game_tick_advances() {
        let mut state = generate(&MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed: 42,
        });
        let mut agents: Vec<Box<dyn Agent>> =
            vec![Box::new(SpreadAgent::new()), Box::new(SpreadAgent::new())];
        run_game(&mut state, &mut agents, 100);
        assert!(state.tick > 0, "ticks should have advanced");
    }
}
