# V3 Personality Report

## Run metadata

- Generated at Unix time `1776149166` from git `0b11328` (`dirty` worktree)
- Command: `simulate_everything_cli v3bench --personality-report`
- Seeds: `0-99`
- Max ticks: `2000`
- Map size: `20x20`
- Snapshot interval: `100` ticks
- Games: `900`

## Personality summary

| Personality | Games | Wins | Draws | Losses | Win rate | Draw rate | Avg ticks | Avg deaths | Avg final entities | Avg final soldiers | Avg final territory |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| spread | 600 | 0 | 600 | 0 | 0.0% | 100.0% | 2000.0 | 2.24 | 34.41 | 4.41 | 2.17 |
| striker | 600 | 0 | 600 | 0 | 0.0% | 100.0% | 2000.0 | 5.73 | 31.24 | 11.24 | 4.32 |
| turtle | 600 | 0 | 600 | 0 | 0.0% | 100.0% | 2000.0 | 1.16 | 34.79 | 5.79 | 2.02 |

## Matchup summary

| Matchup | Games | Wins | Draws | Avg ticks | Avg deaths | Avg final entities | Avg final soldiers | Avg final territory |
|---|---:|---|---:|---:|---:|---|---|---|
| spread-vs-spread | 100 | 0 / 0 | 100 | 2000.0 | 0.00 | 35.00 / 35.00 | 5.00 / 5.00 | 2.21 / 2.17 |
| spread-vs-striker | 100 | 0 / 0 | 100 | 2000.0 | 9.09 | 33.76 / 27.15 | 3.76 / 7.15 | 2.11 / 3.83 |
| spread-vs-turtle | 100 | 0 / 0 | 100 | 2000.0 | 0.00 | 35.00 / 35.00 | 5.00 / 6.00 | 2.21 / 2.17 |
| striker-vs-spread | 100 | 0 / 0 | 100 | 2000.0 | 4.33 | 32.99 / 32.68 | 12.99 / 2.68 | 4.49 / 2.14 |
| striker-vs-striker | 100 | 0 / 0 | 100 | 2000.0 | 7.01 | 33.72 / 29.27 | 13.72 / 9.27 | 4.40 / 4.12 |
| striker-vs-turtle | 100 | 0 / 0 | 100 | 2000.0 | 2.01 | 33.79 / 34.20 | 13.79 / 5.20 | 4.81 / 1.77 |
| turtle-vs-spread | 100 | 0 / 0 | 100 | 2000.0 | 0.00 | 35.00 / 35.00 | 6.00 / 5.00 | 2.21 / 2.17 |
| turtle-vs-striker | 100 | 0 / 0 | 100 | 2000.0 | 4.93 | 34.54 / 30.53 | 5.54 / 10.53 | 1.58 / 4.24 |
| turtle-vs-turtle | 100 | 0 / 0 | 100 | 2000.0 | 0.00 | 35.00 / 35.00 | 6.00 / 6.00 | 2.21 / 2.17 |

## Findings

- striker produces the highest average deaths across the matrix (5.73).
- striker ends with the largest average surviving soldier count (11.24).
- Zero-death stalemates persist in: spread-vs-spread, spread-vs-turtle, turtle-vs-spread, turtle-vs-turtle.

## Diagnosis by matchup

### spread-vs-spread

- Flags: zero_deaths=`true`, flat_entities=`true`, flat_soldiers=`true`, flat_territory=`true`, attrition_without_resolution=`false`
- No combat deaths recorded across sampled games.
- Entity counts stayed flat across snapshots.
- Soldier counts stayed flat across snapshots.
- Territory estimates stayed flat across snapshots.

### spread-vs-striker

- Flags: zero_deaths=`false`, flat_entities=`false`, flat_soldiers=`false`, flat_territory=`false`, attrition_without_resolution=`true`
- Combat causes attrition but the matchup still times out as draws.

### spread-vs-turtle

- Flags: zero_deaths=`true`, flat_entities=`true`, flat_soldiers=`true`, flat_territory=`true`, attrition_without_resolution=`false`
- No combat deaths recorded across sampled games.
- Entity counts stayed flat across snapshots.
- Soldier counts stayed flat across snapshots.
- Territory estimates stayed flat across snapshots.

### striker-vs-spread

- Flags: zero_deaths=`false`, flat_entities=`false`, flat_soldiers=`false`, flat_territory=`false`, attrition_without_resolution=`true`
- Combat causes attrition but the matchup still times out as draws.

### striker-vs-striker

- Flags: zero_deaths=`false`, flat_entities=`false`, flat_soldiers=`false`, flat_territory=`false`, attrition_without_resolution=`true`
- Combat causes attrition but the matchup still times out as draws.

### striker-vs-turtle

- Flags: zero_deaths=`false`, flat_entities=`false`, flat_soldiers=`false`, flat_territory=`false`, attrition_without_resolution=`true`
- Combat causes attrition but the matchup still times out as draws.

### turtle-vs-spread

- Flags: zero_deaths=`true`, flat_entities=`true`, flat_soldiers=`true`, flat_territory=`true`, attrition_without_resolution=`false`
- No combat deaths recorded across sampled games.
- Entity counts stayed flat across snapshots.
- Soldier counts stayed flat across snapshots.
- Territory estimates stayed flat across snapshots.

### turtle-vs-striker

- Flags: zero_deaths=`false`, flat_entities=`false`, flat_soldiers=`false`, flat_territory=`false`, attrition_without_resolution=`true`
- Combat causes attrition but the matchup still times out as draws.

### turtle-vs-turtle

- Flags: zero_deaths=`true`, flat_entities=`true`, flat_soldiers=`true`, flat_territory=`true`, attrition_without_resolution=`false`
- No combat deaths recorded across sampled games.
- Entity counts stayed flat across snapshots.
- Soldier counts stayed flat across snapshots.
- Territory estimates stayed flat across snapshots.

