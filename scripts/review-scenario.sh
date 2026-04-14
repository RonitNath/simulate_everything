#!/usr/bin/env bash
# Run a v3behavior scenario in forensic mode and build a review bundle.
#
# Usage:
#   ./scripts/review-scenario.sh solo_farmer_harvest
#   ./scripts/review-scenario.sh 1v1_sword_engagement
#   ./scripts/review-scenario.sh patrol_responds_to_threat
#   ./scripts/review-scenario.sh settlement_stability_200
#
# Output: var/v3reviews/<scenario>/review.html
# Open in any browser — arrow keys to step, space to play.

set -euo pipefail

SCENARIO="${1:?Usage: $0 <scenario-name>}"
OUT_DIR="var/v3reviews"
SCENARIO_DIR="$OUT_DIR/$SCENARIO"

echo "=== Running $SCENARIO (forensic) ==="
cargo run -q -p simulate-everything-cli --bin simulate_everything_cli -- \
    v3behavior --scenario "$SCENARIO" --forensic --out "$OUT_DIR"

echo "=== Building review bundle ==="
./scripts/build-review-bundle.sh "$SCENARIO_DIR"

echo ""
echo "Done. Open:"
echo "  file://$PWD/$SCENARIO_DIR/review.html"
