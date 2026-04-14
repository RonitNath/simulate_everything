#!/usr/bin/env bash
# Build a self-contained HTML review bundle from v3behavior forensic output.
#
# Usage: ./scripts/build-review-bundle.sh var/v3behavior_demo/settlement_stability_200
#        → produces var/v3behavior_demo/settlement_stability_200/review.html
#
# The bundle embeds all frame PNGs as base64 data URIs and includes
# entity timelines, invariant results, and keyboard navigation.
# Open in any browser — no server needed.
#
# Optional: if ffmpeg is on PATH, also produces review.mp4.

set -euo pipefail

DIR="${1:?Usage: $0 <scenario-output-dir>}"
FRAMES_DIR="$DIR/frames"
SUMMARY="$DIR/summary.json"
TIMELINES="$DIR/entity_timelines.json"
INVARIANTS="$DIR/invariants.json"
OUTPUT="$DIR/review.html"

if [ ! -d "$FRAMES_DIR" ]; then
    echo "No frames/ directory in $DIR — run v3behavior --forensic first"
    exit 1
fi

FRAME_COUNT=$(ls -1 "$FRAMES_DIR"/*.png 2>/dev/null | wc -l)
if [ "$FRAME_COUNT" -eq 0 ]; then
    echo "No PNG frames in $FRAMES_DIR"
    exit 1
fi

echo "Building review bundle: $FRAME_COUNT frames"

# Build base64-encoded frame array
FRAMES_JSON="["
FIRST=1
for f in $(ls -1 "$FRAMES_DIR"/*.png | sort); do
    if [ $FIRST -eq 0 ]; then FRAMES_JSON+=","; fi
    FIRST=0
    B64=$(base64 -w0 "$f")
    FRAMES_JSON+="\"data:image/png;base64,$B64\""
done
FRAMES_JSON+="]"

# Read metadata files if they exist
SUMMARY_JSON="{}"
[ -f "$SUMMARY" ] && SUMMARY_JSON=$(cat "$SUMMARY")

TIMELINES_JSON="{}"
[ -f "$TIMELINES" ] && TIMELINES_JSON=$(cat "$TIMELINES")

INVARIANTS_JSON="[]"
[ -f "$INVARIANTS" ] && INVARIANTS_JSON=$(cat "$INVARIANTS")

cat > "$OUTPUT" <<'HTMLEOF'
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>V3 Behavior Review</title>
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    background: #1a1a2e; color: #e0e0e0;
    font-family: 'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace;
    display: flex; flex-direction: column; height: 100vh;
    overflow: hidden;
  }
  header {
    background: #16213e; padding: 8px 16px;
    display: flex; align-items: center; gap: 16px;
    border-bottom: 1px solid #0f3460;
    flex-shrink: 0;
  }
  header h1 { font-size: 14px; color: #e94560; font-weight: 600; }
  .controls {
    display: flex; align-items: center; gap: 12px; font-size: 13px;
  }
  .controls button {
    background: #0f3460; border: 1px solid #533483;
    color: #e0e0e0; padding: 4px 12px; cursor: pointer;
    font-family: inherit; font-size: 12px; border-radius: 3px;
  }
  .controls button:hover { background: #533483; }
  .controls button.active { background: #e94560; border-color: #e94560; }
  .tick-display {
    background: #0f3460; padding: 4px 10px; border-radius: 3px;
    min-width: 120px; text-align: center; font-size: 13px;
  }
  .speed-label { font-size: 11px; color: #888; }
  main {
    flex: 1; display: flex; overflow: hidden;
  }
  .canvas-wrap {
    flex: 1; display: flex; align-items: center; justify-content: center;
    background: #111; position: relative; overflow: hidden;
  }
  .canvas-wrap img {
    max-width: 100%; max-height: 100%; image-rendering: pixelated;
  }
  .sidebar {
    width: 340px; background: #16213e; border-left: 1px solid #0f3460;
    overflow-y: auto; padding: 12px; font-size: 12px;
    flex-shrink: 0;
  }
  .sidebar h2 { font-size: 12px; color: #e94560; margin-bottom: 6px; text-transform: uppercase; letter-spacing: 1px; }
  .sidebar section { margin-bottom: 16px; }
  .invariant { padding: 3px 0; }
  .invariant.pass { color: #2ecc71; }
  .invariant.fail { color: #e74c3c; }
  .timeline-entity { margin-bottom: 8px; }
  .timeline-entity summary { cursor: pointer; color: #aaa; }
  .timeline-entry { padding: 1px 0; color: #ccc; font-size: 11px; }
  .timeline-entry.current { color: #e94560; font-weight: bold; }
  .scrubber {
    width: 100%; height: 24px; cursor: pointer; flex-shrink: 0;
    background: #16213e; border-top: 1px solid #0f3460;
  }
  .scrubber input {
    width: 100%; height: 100%; -webkit-appearance: none; appearance: none;
    background: transparent; outline: none;
  }
  .scrubber input::-webkit-slider-thumb {
    -webkit-appearance: none; width: 3px; height: 24px;
    background: #e94560; cursor: grab;
  }
  .scrubber input::-webkit-slider-runnable-track {
    height: 24px; background: #0f3460;
  }
  .kb-hint { font-size: 10px; color: #555; }
</style>
</head>
<body>
<header>
  <h1>V3 BEHAVIOR REVIEW</h1>
  <div class="controls">
    <button id="btn-prev" title="Previous frame (←)">◄</button>
    <button id="btn-play" title="Play/Pause (Space)">▶</button>
    <button id="btn-next" title="Next frame (→)">►</button>
    <div class="tick-display" id="tick-display">tick 0 / 0</div>
    <button id="btn-slower">-</button>
    <span class="speed-label" id="speed-label">10 fps</span>
    <button id="btn-faster">+</button>
    <span class="kb-hint">← → Space +/- Home End</span>
  </div>
</header>
<main>
  <div class="canvas-wrap">
    <img id="frame-img" alt="frame">
  </div>
  <div class="sidebar" id="sidebar">
    <section id="summary-section"><h2>Summary</h2><pre id="summary-text"></pre></section>
    <section id="invariants-section"><h2>Invariants</h2><div id="invariants-list"></div></section>
    <section id="timeline-section"><h2>Entity Timelines</h2><div id="timeline-list"></div></section>
  </div>
</main>
<div class="scrubber"><input type="range" id="scrubber" min="0" max="0" value="0"></div>
<script>
HTMLEOF

# Inject data
echo "const FRAMES = $FRAMES_JSON;" >> "$OUTPUT"
echo "const SUMMARY = $SUMMARY_JSON;" >> "$OUTPUT"
echo "const TIMELINES = $TIMELINES_JSON;" >> "$OUTPUT"
echo "const INVARIANTS = $INVARIANTS_JSON;" >> "$OUTPUT"

cat >> "$OUTPUT" <<'JSEOF'
let currentFrame = 0;
let playing = false;
let fps = 10;
let playInterval = null;

const img = document.getElementById('frame-img');
const tickDisplay = document.getElementById('tick-display');
const scrubber = document.getElementById('scrubber');
const speedLabel = document.getElementById('speed-label');
const btnPlay = document.getElementById('btn-play');

scrubber.max = FRAMES.length - 1;

function showFrame(idx) {
  idx = Math.max(0, Math.min(FRAMES.length - 1, idx));
  currentFrame = idx;
  img.src = FRAMES[idx];
  tickDisplay.textContent = `tick ${idx} / ${FRAMES.length - 1}`;
  scrubber.value = idx;
  highlightTimelines(idx);
}

function togglePlay() {
  playing = !playing;
  btnPlay.textContent = playing ? '⏸' : '▶';
  btnPlay.classList.toggle('active', playing);
  if (playing) {
    playInterval = setInterval(() => {
      if (currentFrame >= FRAMES.length - 1) { togglePlay(); return; }
      showFrame(currentFrame + 1);
    }, 1000 / fps);
  } else {
    clearInterval(playInterval);
  }
}

function setFps(newFps) {
  fps = Math.max(1, Math.min(60, newFps));
  speedLabel.textContent = `${fps} fps`;
  if (playing) {
    clearInterval(playInterval);
    playInterval = setInterval(() => {
      if (currentFrame >= FRAMES.length - 1) { togglePlay(); return; }
      showFrame(currentFrame + 1);
    }, 1000 / fps);
  }
}

// Summary
document.getElementById('summary-text').textContent = JSON.stringify(SUMMARY, null, 2);

// Invariants
const invList = document.getElementById('invariants-list');
if (Array.isArray(INVARIANTS)) {
  INVARIANTS.forEach(inv => {
    const div = document.createElement('div');
    div.className = `invariant ${inv.passed ? 'pass' : 'fail'}`;
    div.textContent = `${inv.passed ? '✓' : '✗'} ${inv.name}`;
    invList.appendChild(div);
  });
}

// Timelines
const tlList = document.getElementById('timeline-list');
const timelineEntries = {};
if (typeof TIMELINES === 'object' && TIMELINES !== null) {
  Object.entries(TIMELINES).forEach(([entityId, points]) => {
    const details = document.createElement('details');
    const summary = document.createElement('summary');
    summary.textContent = `Entity ${entityId} (${points.length} points)`;
    details.appendChild(summary);
    const entries = [];
    points.forEach(pt => {
      const div = document.createElement('div');
      div.className = 'timeline-entry';
      div.textContent = `t${pt.tick}: ${pt.goal || '-'} → ${pt.action || '-'}`;
      div.dataset.tick = pt.tick;
      details.appendChild(div);
      entries.push(div);
    });
    tlList.appendChild(details);
    timelineEntries[entityId] = entries;
  });
}

function highlightTimelines(tick) {
  Object.values(timelineEntries).forEach(entries => {
    entries.forEach(div => {
      div.classList.toggle('current', parseInt(div.dataset.tick) === tick);
    });
  });
}

// Controls
document.getElementById('btn-prev').onclick = () => showFrame(currentFrame - 1);
document.getElementById('btn-next').onclick = () => showFrame(currentFrame + 1);
document.getElementById('btn-play').onclick = togglePlay;
document.getElementById('btn-slower').onclick = () => setFps(fps - 2);
document.getElementById('btn-faster').onclick = () => setFps(fps + 2);
scrubber.oninput = () => showFrame(parseInt(scrubber.value));

document.addEventListener('keydown', e => {
  switch(e.key) {
    case 'ArrowLeft': showFrame(currentFrame - 1); break;
    case 'ArrowRight': showFrame(currentFrame + 1); break;
    case ' ': e.preventDefault(); togglePlay(); break;
    case '+': case '=': setFps(fps + 2); break;
    case '-': setFps(fps - 2); break;
    case 'Home': showFrame(0); break;
    case 'End': showFrame(FRAMES.length - 1); break;
  }
});

showFrame(0);
</script>
</body>
</html>
JSEOF

echo "Review bundle written to $OUTPUT ($FRAME_COUNT frames)"
echo "Open in a browser: file://$PWD/$OUTPUT"

# Optional: ffmpeg video
if command -v ffmpeg &>/dev/null; then
    VIDEO="$DIR/review.mp4"
    echo "Building video with ffmpeg..."
    ffmpeg -y -framerate 10 -pattern_type glob -i "$FRAMES_DIR/*.png" \
        -c:v libx264 -pix_fmt yuv420p -crf 23 "$VIDEO" 2>/dev/null
    echo "Video written to $VIDEO"
fi
