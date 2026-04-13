use super::AGENT_POLL_INTERVAL;
use super::agent::Agent;
use super::directive;
use super::observation;
use super::sim;
use super::state::GameState;

pub(crate) fn advance_game_tick(state: &mut GameState, agents: &mut [Box<dyn Agent>]) {
    if state.tick % AGENT_POLL_INTERVAL as u64 == 0 {
        for (player_id, agent) in agents.iter_mut().enumerate() {
            let pid = player_id as u8;
            if !state.players.iter().any(|p| p.id == pid && p.alive) {
                continue;
            }
            let obs = observation::observe(state, pid);
            let directives = agent.act(&obs);
            tracing::trace!(
                tick = state.tick,
                player = pid,
                directives = directives.len(),
                "agent polled"
            );
            directive::apply_directives(state, pid, &directives);
        }
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
    while state.tick < tick_limit && !sim::is_over(state) {
        if state.tick % AGENT_POLL_INTERVAL as u64 == 0 {
            for (player_id, agent) in agents.iter_mut().enumerate() {
                let pid = player_id as u8;
                if !state.players.iter().any(|p| p.id == pid && p.alive) {
                    continue;
                }
                let obs = observation::observe(state, pid);
                let directives = agent.act(&obs);
                tracing::trace!(
                    tick = state.tick,
                    player = pid,
                    directives = directives.len(),
                    "agent polled"
                );
                directive::apply_directives(state, pid, &directives);
            }
        }

        sim::tick(state);
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
                let units: Vec<_> = state.units.iter().filter(|u| u.owner == p.id).collect();
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
