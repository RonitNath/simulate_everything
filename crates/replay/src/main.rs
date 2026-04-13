use generals_engine::agent::{self, Agent};
use generals_engine::game::Game;
use generals_engine::mapgen::{self, MapConfig};
use generals_engine::replay::Replay;
use generals_engine::state::Tile;
use rand::rngs::StdRng;
use rand::SeedableRng;
use rayon::prelude::*;
use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};

const VIEWER_CSS: &str = include_str!("viewer.css");
const VIEWER_JS: &str = include_str!("viewer.js");

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    if args.iter().any(|a| a == "serve") {
        #[cfg(feature = "serve")]
        serve_main(&args);
        #[cfg(not(feature = "serve"))]
        {
            eprintln!("error: built without 'serve' feature. Rebuild with --features serve");
            std::process::exit(1);
        }
        return;
    }

    export_main(&args);
}

fn print_usage() {
    eprintln!(
        r#"simulate_everything_replay — generate and serve game replays

EXPORT MODE (default):
  simulate_everything_replay [options]
    --seeds 100-110       Seed range or comma-separated list (default: 42)
    --agents a,b          Agent names (default: pressure,swarm)
    --turns N             Max turns per game (default: 500)
    --size WxH            Board dimensions (default: auto for player count)
    --out DIR             Output directory (default: ./replays)
    --format json|html    Output format (default: html)
    --title TEXT          Title for HTML pages (default: "simulate_everything Replay")

SERVE MODE:
  simulate_everything_replay serve [options]
    --dir DIR             Directory containing replay files (default: ./replays)
    --port N              Port to listen on (default: 8080)
    --bind ADDR           Bind address (default: 127.0.0.1)"#
    );
}

// ---------------------------------------------------------------------------
// Export mode
// ---------------------------------------------------------------------------

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

fn parse_seed_range(s: &str) -> Vec<u64> {
    if let Some((a, b)) = s.split_once('-') {
        let start: u64 = a.parse().expect("bad seed range start");
        let end: u64 = b.parse().expect("bad seed range end");
        (start..=end).collect()
    } else {
        s.split(',')
            .map(|x| x.trim().parse::<u64>().expect("bad seed"))
            .collect()
    }
}

fn parse_size(s: &str) -> (usize, usize) {
    let (w, h) = s.split_once('x').expect("size must be WxH");
    (w.parse().expect("bad width"), h.parse().expect("bad height"))
}

fn export_main(args: &[String]) {
    let seeds = flag_value(args, "--seeds")
        .map(parse_seed_range)
        .unwrap_or_else(|| vec![42]);

    let agent_names_str = flag_value(args, "--agents").unwrap_or("pressure,swarm");
    let agent_names: Vec<&str> = agent_names_str.split(',').collect();

    let max_turns: u32 = flag_value(args, "--turns")
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);

    let size = flag_value(args, "--size").map(parse_size);
    let out_dir = PathBuf::from(flag_value(args, "--out").unwrap_or("./replays"));
    let format = flag_value(args, "--format").unwrap_or("html");
    let title = flag_value(args, "--title").unwrap_or("simulate_everything Replay");

    // Validate agents.
    for name in &agent_names {
        if agent::agent_by_name(name).is_none() {
            eprintln!(
                "error: unknown agent '{}'. known: {:?}",
                name,
                agent::builtin_agent_names()
            );
            std::process::exit(1);
        }
    }

    let num_players = agent_names.len() as u8;
    let (w, h) = size.unwrap_or_else(|| {
        let cfg = MapConfig::for_players(num_players);
        (cfg.width, cfg.height)
    });

    std::fs::create_dir_all(&out_dir).expect("failed to create output directory");

    eprintln!(
        "Generating {} replays: {} on {}x{} (max_turns={})",
        seeds.len(),
        agent_names.join(" vs "),
        w, h, max_turns,
    );

    // Generate replays in parallel.
    let replays: Vec<(u64, Replay)> = seeds
        .par_iter()
        .map(|&seed| {
            let replay = generate_replay(seed, &agent_names, max_turns, (w, h));
            (seed, replay)
        })
        .collect();

    // Write output files.
    for (seed, replay) in &replays {
        let matchup = agent_names.join("-vs-");
        let filename = format!("{}_seed{}", matchup, seed);

        match format {
            "json" => {
                let path = out_dir.join(format!("{}.json", filename));
                let json = serde_json::to_string(replay).unwrap();
                std::fs::write(&path, json).unwrap();
                eprintln!("  wrote {}", path.display());
            }
            "html" => {
                let path = out_dir.join(format!("{}.html", filename));
                let html = render_standalone_html(replay, *seed, &agent_names, title);
                std::fs::write(&path, html).unwrap();
                eprintln!("  wrote {}", path.display());
            }
            _ => {
                eprintln!("error: unknown format '{}'. use json or html", format);
                std::process::exit(1);
            }
        }
    }

    // Write index if multiple replays.
    if replays.len() > 1 {
        let index_path = out_dir.join("index.html");
        let index_html = render_index_html(&replays, &agent_names, title, format);
        std::fs::write(&index_path, index_html).unwrap();
        eprintln!("  wrote {}", index_path.display());
    }

    eprintln!("Done. {} files in {}", replays.len(), out_dir.display());
}

fn generate_replay(
    seed: u64,
    agent_names: &[&str],
    max_turns: u32,
    (w, h): (usize, usize),
) -> Replay {
    let num_players = agent_names.len() as u8;
    let mut rng = StdRng::seed_from_u64(seed);
    let config = MapConfig::for_size(w, h, num_players);
    let state = mapgen::generate(&config, &mut rng);

    let mut agents: Vec<Box<dyn Agent>> = agent_names
        .iter()
        .map(|name| agent::agent_by_name(name).unwrap())
        .collect();

    let ids: Vec<String> = agents.iter().map(|a| a.id()).collect();
    let mut replay = Replay::new(&state, ids);
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

// ---------------------------------------------------------------------------
// Compact replay format — delta-encoded with periodic keyframes
// ---------------------------------------------------------------------------

/// Serialize a Replay into a compact JSON format for the viewer.
///
/// Format:
/// - `w`, `h`: board dimensions
/// - `np`: number of players
/// - `an`: agent names array
/// - `wi`: winner player index (-1 = draw)
/// - `ki`: keyframe interval
/// - Frames `f` array where each frame has:
///   - `t`: turn number
///   - `s`: stats as `[[player, land, armies, alive], ...]`
///   - `g`: (keyframes only) interleaved `[tile,owner,armies,...]` for all cells
///   - `d`: (delta frames) flat `[idx,tile,owner,armies,...]` for changed cells only
fn compact_replay_json(replay: &Replay, keyframe_interval: usize) -> String {
    let total = replay.width * replay.height;
    let est_size = total * 10 + replay.frames.len() * 200;
    let mut out = String::with_capacity(est_size);

    out.push_str("{\"w\":");
    write!(out, "{}", replay.width).unwrap();
    out.push_str(",\"h\":");
    write!(out, "{}", replay.height).unwrap();
    out.push_str(",\"np\":");
    write!(out, "{}", replay.num_players).unwrap();
    out.push_str(",\"an\":[");
    for (i, name) in replay.agent_names.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push('"');
        out.push_str(name);
        out.push('"');
    }
    out.push_str("],\"wi\":");
    write!(out, "{}", replay.winner.map_or(-1i32, |w| w as i32)).unwrap();
    out.push_str(",\"ki\":");
    write!(out, "{}", keyframe_interval).unwrap();
    out.push_str(",\"f\":[");

    let mut prev_grid: Option<&[generals_engine::state::Cell]> = None;

    for (i, frame) in replay.frames.iter().enumerate() {
        if i > 0 { out.push(','); }

        let is_keyframe = i % keyframe_interval == 0;

        out.push_str("{\"t\":");
        write!(out, "{}", frame.turn).unwrap();

        if is_keyframe || prev_grid.is_none() {
            out.push_str(",\"g\":[");
            for (j, cell) in frame.grid.iter().enumerate() {
                if j > 0 { out.push(','); }
                write_cell_compact(&mut out, cell);
            }
            out.push(']');
        } else if let Some(prev) = prev_grid {
            out.push_str(",\"d\":[");
            let mut first = true;
            for (j, cell) in frame.grid.iter().enumerate() {
                let p = &prev[j];
                if cell.tile != p.tile || cell.owner != p.owner || cell.armies != p.armies {
                    if !first { out.push(','); }
                    write!(out, "{},", j).unwrap();
                    write_cell_compact(&mut out, cell);
                    first = false;
                }
            }
            out.push(']');
        }

        out.push_str(",\"s\":[");
        for (j, stat) in frame.stats.iter().enumerate() {
            if j > 0 { out.push(','); }
            write!(out, "[{},{},{},{}]",
                stat.player, stat.land, stat.armies,
                if stat.alive { 1 } else { 0 }
            ).unwrap();
        }
        out.push_str("]}");

        prev_grid = Some(&frame.grid);
    }

    out.push_str("]}");
    out
}

fn write_cell_compact(out: &mut String, cell: &generals_engine::state::Cell) {
    let tile: u8 = match cell.tile {
        Tile::Empty => 0,
        Tile::Mountain => 1,
        Tile::City => 2,
        Tile::General => 3,
    };
    let owner = cell.owner.map_or(-1i32, |o| o as i32);
    write!(out, "{},{},{}", tile, owner, cell.armies).unwrap();
}

// ---------------------------------------------------------------------------
// Self-contained HTML renderer
// ---------------------------------------------------------------------------

fn render_standalone_html(
    replay: &Replay,
    seed: u64,
    agent_names: &[&str],
    title: &str,
) -> String {
    let keyframe_interval = if replay.width * replay.height > 10_000 { 50 } else { 100 };
    let replay_json = compact_replay_json(replay, keyframe_interval);
    // For many players, show "12p FFA (pressure, expander, swarm)" instead of listing all
    let matchup = if agent_names.len() > 4 {
        let mut unique: Vec<&str> = agent_names.to_vec();
        unique.sort();
        unique.dedup();
        format!("{}p FFA ({})", agent_names.len(), unique.join(", "))
    } else {
        agent_names.join(" vs ")
    };
    let winner_text = match replay.winner {
        Some(w) => format!("Winner: {}", replay.agent_names[w as usize]),
        None => "Draw".to_string(),
    };
    let total_turns = replay.frames.len().saturating_sub(1);
    let title_esc = html_escape(title);
    let matchup_esc = html_escape(&matchup);
    let winner_esc = html_escape(&winner_text);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - {matchup} (seed {seed})</title>
<style>
{css}
</style>
</head>
<body>
<div id="app">
  <div class="header">
    <span class="title">{title}</span>
    <span class="meta">{matchup} &middot; seed {seed} &middot; {winner} in {turns} turns</span>
  </div>
  <div class="controls">
    <button id="btn-start">&#x23EE;</button>
    <button id="btn-prev">&#x23F4;</button>
    <button id="btn-play">&#x25B6;</button>
    <button id="btn-next">&#x23F5;</button>
    <button id="btn-end">&#x23ED;</button>
    <span id="turn-label">Turn 0 / {turns}</span>
    <input type="range" id="scrubber" min="0" max="{turns}" value="0">
  </div>
  <div class="speed-bar">
    <span>Speed:</span>
    <button class="speed-btn" data-speed="1">1x</button>
    <button class="speed-btn" data-speed="5">5x</button>
    <button class="speed-btn active" data-speed="10">10x</button>
    <button class="speed-btn" data-speed="25">25x</button>
    <button class="speed-btn" data-speed="50">50x</button>
    <button class="speed-btn" data-speed="100">100x</button>
    <span style="margin-left:auto"></span>
    <button id="btn-numbers" class="speed-btn">&#x0023;&#x0338;</button>
  </div>
  <div class="main">
    <div class="board-container"><canvas id="board"></canvas></div>
    <div class="sidebar" id="stats"></div>
  </div>
</div>
<script>
var REPLAY = {json};
</script>
<script>
{js}
</script>
</body>
</html>"#,
        title = title_esc,
        matchup = matchup_esc,
        seed = seed,
        winner = winner_esc,
        turns = total_turns,
        css = VIEWER_CSS,
        json = replay_json,
        js = VIEWER_JS,
    )
}

fn render_index_html(
    replays: &[(u64, Replay)],
    agent_names: &[&str],
    title: &str,
    format: &str,
) -> String {
    let matchup = agent_names.join(" vs ");
    let ext = if format == "json" { "json" } else { "html" };
    let matchup_slug = agent_names.join("-vs-");

    let mut rows = String::new();
    for (seed, replay) in replays {
        let winner_text = match replay.winner {
            Some(w) => replay.agent_names[w as usize].clone(),
            None => "Draw".to_string(),
        };
        let turns = replay.frames.len().saturating_sub(1);
        let filename = format!("{}_seed{}.{}", matchup_slug, seed, ext);
        rows.push_str(&format!(
            "<tr><td><a href=\"{filename}\">{seed}</a></td><td>{winner}</td><td>{turns}</td></tr>",
            filename = filename,
            seed = seed,
            winner = html_escape(&winner_text),
            turns = turns,
        ));
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{title} - {matchup}</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ background: #0a0a0f; color: #e0e0e8; font-family: 'JetBrains Mono', monospace; padding: 24px; }}
h1 {{ font-size: 18px; margin-bottom: 16px; }}
table {{ border-collapse: collapse; width: 100%; max-width: 600px; }}
th, td {{ text-align: left; padding: 8px 16px; border-bottom: 1px solid #2a2a3e; }}
th {{ color: #8888a0; font-size: 10px; text-transform: uppercase; letter-spacing: 0.05em; border-bottom-width: 2px; }}
a {{ color: #4a9eff; text-decoration: none; }}
a:hover {{ text-decoration: underline; }}
</style>
</head>
<body>
<h1>{title} - {matchup}</h1>
<table>
<thead><tr><th>Seed</th><th>Winner</th><th>Turns</th></tr></thead>
<tbody>{rows}</tbody>
</table>
</body>
</html>"#,
        title = html_escape(title),
        matchup = html_escape(&matchup),
        rows = rows,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// Serve mode
// ---------------------------------------------------------------------------

#[cfg(feature = "serve")]
fn serve_main(args: &[String]) {
    use axum::{routing::get_service, Router};
    use tower_http::services::ServeDir;

    let dir = flag_value(args, "--dir").unwrap_or("./replays");
    let port: u16 = flag_value(args, "--port")
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let bind = flag_value(args, "--bind").unwrap_or("127.0.0.1");
    let addr = format!("{}:{}", bind, port);

    if !Path::new(dir).exists() {
        eprintln!("error: directory '{}' does not exist", dir);
        std::process::exit(1);
    }

    eprintln!("Serving replays from {} on http://{}", dir, addr);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let app = Router::new().fallback_service(get_service(ServeDir::new(dir)));
        let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
        axum::serve(listener, app).await.unwrap();
    });
}
