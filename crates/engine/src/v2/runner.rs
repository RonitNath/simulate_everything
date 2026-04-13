use super::AGENT_POLL_INTERVAL;
use super::agent::Agent;
use super::directive;
use super::observation;
use super::sim;
use super::state::GameState;

/// Run a complete game with the given agents, polling each every AGENT_POLL_INTERVAL ticks.
/// Agents are indexed by player ID: agents[0] controls player 0, etc.
/// Returns the winner's player ID, or None for a draw / simultaneous elimination.
pub fn run_game(
    state: &mut GameState,
    agents: &mut [Box<dyn Agent>],
    max_ticks: u64,
) -> Option<u8> {
    while state.tick < max_ticks && !sim::is_over(state) {
        if state.tick % AGENT_POLL_INTERVAL as u64 == 0 {
            for (player_id, agent) in agents.iter_mut().enumerate() {
                let pid = player_id as u8;
                // Skip eliminated players
                if !state.players.iter().any(|p| p.id == pid && p.alive) {
                    continue;
                }
                let obs = observation::observe(state, pid);
                let directives = agent.act(&obs);
                directive::apply_directives(state, pid, &directives);
            }
        }

        sim::tick(state);
    }

    sim::winner(state)
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
        let mut agents: Vec<Box<dyn Agent>> = vec![Box::new(SpreadAgent), Box::new(SpreadAgent)];
        run_game(&mut state, &mut agents, 5000);
        assert!(sim::is_over(&state), "game should be over after run_game");
    }

    #[test]
    fn run_game_returns_winner() {
        let mut state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 100,
        });
        let mut agents: Vec<Box<dyn Agent>> = vec![Box::new(SpreadAgent), Box::new(SpreadAgent)];
        // Either a winner or a draw — no panics either way
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
            Box::new(SpreadAgent),
            Box::new(SpreadAgent),
            Box::new(SpreadAgent),
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
        let mut agents: Vec<Box<dyn Agent>> = vec![Box::new(SpreadAgent), Box::new(SpreadAgent)];
        run_game(&mut state, &mut agents, 100);
        assert!(state.tick > 0, "ticks should have advanced");
    }
}
