#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

default_static_dir="$repo_root/frontend/dist"
default_viewer_dir="$repo_root/crates/viewer/dist"
export SIMEV_BIND_ADDR="${SIMEV_BIND_ADDR:-127.0.0.1}"
export SIMEV_PORT="${SIMEV_PORT:-3334}"
export SIMEV_STATIC_DIR="${SIMEV_STATIC_DIR:-$default_static_dir}"
export SIMEV_VIEWER_DIR="${SIMEV_VIEWER_DIR:-$default_viewer_dir}"
export SIMEV_V2_RR_REVIEW_DIR="${SIMEV_V2_RR_REVIEW_DIR:-$repo_root/var/dev_v2_rr_reviews}"
export SIMEV_V3_RR_REVIEW_DIR="${SIMEV_V3_RR_REVIEW_DIR:-$repo_root/var/dev_v3_reviews}"

mkdir -p "$SIMEV_V2_RR_REVIEW_DIR" "$SIMEV_V3_RR_REVIEW_DIR"

cat <<EOF
Starting isolated simulate_everything dev backend
  repo: $repo_root
  bind: http://$SIMEV_BIND_ADDR:$SIMEV_PORT
  static dir: $SIMEV_STATIC_DIR
  viewer dir: $SIMEV_VIEWER_DIR
  v2 reviews: $SIMEV_V2_RR_REVIEW_DIR
  v3 reviews: $SIMEV_V3_RR_REVIEW_DIR

Viewer URL:
  http://$SIMEV_BIND_ADDR:$SIMEV_PORT/v3/rr
EOF

if [[ ! -d "$SIMEV_STATIC_DIR" ]]; then
  echo "warning: static dir does not exist yet: $SIMEV_STATIC_DIR" >&2
  echo "build the frontend first if you need the web shell served from this backend" >&2
fi

if [[ ! -d "$SIMEV_VIEWER_DIR" ]]; then
  echo "warning: viewer dir does not exist yet: $SIMEV_VIEWER_DIR" >&2
  echo "run 'cd crates/viewer && trunk build' before using /v3/rr" >&2
fi

exec cargo run -p simulate-everything-web
