# Spec: V3 Review Gallery

## Vision

A single frontend page at `/v3/reviews` where all behavior forensics and RR review
bundles surface automatically. Cards show metadata at a glance (scenario name, agents,
pass/fail, filmstrip thumbnail). Clicking a card expands it inline (accordion/dropdown)
with an iframe loading the bundle's self-contained `review.html`. No inbox, no workflow
state — bundles appear as soon as they exist on disk.

## Use Cases

1. **Browse available bundles** — Ronit opens `/v3/reviews` and sees all review bundles
   grouped by source: behavior forensics scenarios on top, RR captures below. Each card
   shows scenario name (or game/flag ID), agent names, tick count, pass/fail badges for
   invariants, and a filmstrip thumbnail if available. Sorted most-recent first within
   each group (by file modification time).

2. **Expand and review inline** — Clicking a card expands it as a dropdown, revealing an
   iframe that loads the bundle's `review.html`. The iframe is lazy-loaded on expand (not
   on page load — these files are ~65MB). Only one card expanded at a time (accordion
   model — expanding one collapses the previous). All review.html controls work within
   the iframe (arrow keys, space, scrubber).

3. **Collapse and move on** — Clicking the expanded card header collapses it, destroying
   the iframe to free memory.

4. **Refresh** — Page load always re-fetches the bundle list from the API. No caching.
   Generate a new bundle with `./scripts/review-scenario.sh <name>`, refresh the page,
   it appears.

## Architecture

### Components

| Component | Responsibility |
|-----------|---------------|
| `GET /v3/reviews` (Axum) | Serves HTML shell that loads the SolidJS review gallery |
| `GET /api/v3/reviews/all` | Scans both bundle directories, returns unified metadata list |
| `GET /reviews/files/{path+}` | Serves static files from bundle directories (review.html, filmstrip.png) |
| `ReviewGallery.tsx` | Root SolidJS component — fetches bundle list, renders grouped cards |
| `BundleCard.tsx` | Single card — metadata display, expand/collapse, lazy iframe |

### Data Flow

```
Page load → fetch /api/v3/reviews/all
         → returns BundleInfo[] (metadata from summary.json per bundle)
         → render grouped cards

Card click → expand accordion
          → mount <iframe src="/reviews/files/v3behavior_farmer/solo_farmer_harvest/review.html">
          → iframe loads self-contained review player

Card collapse → unmount iframe (free memory)
```

### API Surface

**`GET /api/v3/reviews/all`**

Returns a JSON array of bundle metadata. The backend scans:
- `var/v3behavior_*/*/summary.json` — behavior forensics bundles
- `var/v3_reviews/game_*/*/summary.json` — RR flag/segment captures (existing)

Response shape:
```json
[
  {
    "id": "v3behavior_farmer/solo_farmer_harvest",
    "source": "behavior",
    "name": "solo_farmer_harvest",
    "category": "v3behavior_farmer",
    "agent_names": ["farmer_v1"],
    "tick_count": 220,
    "passed": true,
    "invariants": [
      { "name": "hunger_recovers", "passed": true },
      { "name": "goal_selected_eat", "passed": true }
    ],
    "has_filmstrip": true,
    "modified_at": "2026-04-14T12:34:56Z"
  },
  {
    "id": "v3_reviews/game_5/flag_342",
    "source": "rr_capture",
    "name": "flag_342",
    "category": "game_5",
    "annotation": "weird combat behavior",
    "agent_names": ["striker", "turtle"],
    "tick_count": 101,
    "seed": 12345,
    "modified_at": "2026-04-14T11:00:00Z"
  }
]
```

**`GET /reviews/files/{path+}`**

Static file serving from known bundle root directories. Path must resolve within
`var/v3behavior_*/` or `var/v3_reviews/` — reject traversal attempts.

Used for:
- Filmstrip thumbnails: `/reviews/files/v3behavior_farmer/solo_farmer_harvest/filmstrip.png`
- Review HTML (iframe src): `/reviews/files/v3behavior_farmer/solo_farmer_harvest/review.html`

### UI Surface

**Page layout:**
```
┌─────────────────────────────────────────────┐
│  Review Gallery                    [Refresh] │
├─────────────────────────────────────────────┤
│                                             │
│  Behavior Scenarios                         │
│  ┌─────────────────────────────────────────┐│
│  │ ▶ solo_farmer_harvest                   ││
│  │   farmer_v1 · 220 ticks · ✓ 2/2        ││
│  │   [filmstrip thumbnail]                 ││
│  └─────────────────────────────────────────┘│
│  ┌─────────────────────────────────────────┐│
│  │ ▼ 1v1_sword_engagement          [open] ││
│  │   striker, turtle · 155 ticks · ✗ 1/2   ││
│  │   ┌───────────────────────────────────┐ ││
│  │   │                                   │ ││
│  │   │   <iframe review.html>            │ ││
│  │   │   (full review player)            │ ││
│  │   │                                   │ ││
│  │   └───────────────────────────────────┘ ││
│  └─────────────────────────────────────────┘│
│                                             │
│  RR Captures                                │
│  ┌─────────────────────────────────────────┐│
│  │ ▶ game_5 / flag_342                     ││
│  │   striker, turtle · 101 ticks           ││
│  │   "weird combat behavior"               ││
│  └─────────────────────────────────────────┘│
│                                             │
└─────────────────────────────────────────────┘
```

**Card states:**
- **Collapsed** (default): metadata row + filmstrip thumbnail (if available)
- **Expanded**: metadata row + iframe loading review.html at ~80vh height

**Visual indicators:**
- Pass/fail badge: green checkmark with count for all-pass, red X with count for any failure
- Filmstrip thumbnail shown in collapsed state for behavior bundles (gives visual preview)
- RR captures show annotation text in collapsed state
- Category headers ("Behavior Scenarios", "RR Captures") separate the two sources

### Frontend Entry Point

New Vite entry point `frontend/src/reviews.tsx` alongside existing `frontend/src/index.tsx`.
Axum serves a separate HTML shell at `/v3/reviews` that loads this entry point from
`/static/reviews.js`. This matches the pattern of `/v3/rr` loading `/static/index.js`.

Vite config gets a second entry in `build.rollupOptions.input`.

## Security

**Path traversal on file serving.** The `/reviews/files/{path+}` endpoint must:
- Canonicalize the resolved path
- Verify it falls within one of the two known bundle root directories (`var/v3behavior_*/` or `var/v3_reviews/`)
- Reject `..` segments before resolution

No auth — internal tool on nexus, not publicly routable.

## Privacy

No PII handled. Bundle data is simulation state only.

## Scope

### V1 (ship this)
- `GET /api/v3/reviews/all` endpoint scanning both bundle directories
- `GET /reviews/files/{path+}` static file serving with traversal protection
- `GET /v3/reviews` HTML shell route
- `ReviewGallery.tsx` with accordion card expansion
- `BundleCard.tsx` with metadata display + lazy iframe
- Separate Vite entry point (`reviews.tsx`)
- Filmstrip thumbnails in collapsed cards (behavior bundles only)
- Group by source (behavior vs RR), sort by modified_at descending

### Deferred
- Delete bundles from the gallery UI
- Filter/search controls (by agent name, pass/fail, scenario name)
- Side-by-side comparison of two bundles
- Inline annotations or notes per bundle
- Auto-refresh / polling for new bundles
- Aggregate pass/fail dashboard across all scenarios

## Convention References

- Frontend patterns: `frontend/src/V3App.tsx` (SolidJS component structure, signal patterns)
- Styling: `frontend/src/styles/` (Vanilla Extract with theme tokens)
- Backend routes: `crates/web/src/main.rs` (Axum route wiring)
- Existing review API: `crates/web/src/v3_review.rs`

## Verification

1. Start server, navigate to `/v3/reviews` — page loads, shows empty state or existing bundles
2. Run `./scripts/review-scenario.sh solo_farmer_harvest`, refresh page — new card appears
3. Click card — expands with iframe, review.html player works (arrow keys, space, scrubber)
4. Click another card — previous collapses, new one expands
5. Filmstrip thumbnails render for behavior bundles
6. RR capture bundles (if any exist) appear in separate group
7. Path traversal attempt on `/reviews/files/../../etc/passwd` returns 400/404

## Deploy Strategy

Same binary — code changes are in `crates/web/` and `frontend/`. Rebuild frontend
(`cd frontend && bun run build`), rebuild Rust binary, restart service. No migration,
no new dependencies, no infrastructure changes.

## Files Modified

### New files
- `frontend/src/reviews.tsx` — entry point for review gallery page
- `frontend/src/reviews/ReviewGallery.tsx` — root gallery component
- `frontend/src/reviews/BundleCard.tsx` — individual bundle card with accordion
- `frontend/src/styles/reviews.css.ts` — gallery-specific styles

### Modified files
- `frontend/vite.config.ts` — add second entry point for reviews
- `crates/web/src/main.rs` — add routes: `/v3/reviews`, `/api/v3/reviews/all`, `/reviews/files/*`
- `crates/web/src/v3_review.rs` — add `list_all_bundles()` scanning both directories, add `BundleInfo` type
