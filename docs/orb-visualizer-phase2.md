# Live Orb visualizer: phase 2 plan and review

## Result and scope

Make the accepted Mesh Orb experience from wgputests main (`3c0eb41`) the
production default for the regular Herdr viewer and Windows screensaver/preview.
`--tree` selects the existing tree for either executable. Keep `--port`, `--check`,
Windows /s /p /c conventions, observer service, wire protocol and daemon unchanged.
Push a reviewed branch and open a PR; do not merge (user instruction).

## Functional behavior

- Use the current `client::View` and scoped `projection::Scene` as the only data
  source. No synthetic model, lab controls, random activity or invented identities.
- Represent coordinator, node, session, workspace and agent using the accepted
  geometry, stable territories, depth cues, role palette and visual key.
- Keep a member node distinct from its logical coordinator control role. Retain
  default/named session incarnation and workspace/tab/agent scoping. Label changes
  and snapshot ordering must not move a surviving entity to another slot.
- Preserve idle and unknown agent states as neutral forms, rather than completed.
  Stale/offline/retained observations must be visibly attenuated and identified.
- Preserve the existing dashboard-equivalent Fleet pulse cards and freshness
  rules. Footer counts show all observed nodes, sessions, reported workspaces and
  agents plus working/blocked/done; unresolved workspace placeholders do not count
  as reported workspaces. Counts include all states and are independent of detail
  sampling. Explicitly label retained totals when disconnected.
- Real advancing last-seen receipts drive 3-second node-to-coordinator effects,
  originating through an available child path. No fabricated agent heartbeat.
  Use the existing receipt baselines, epochs, timestamp validation and cleanup.
- Changes to observed agent states produce bounded 10-second callouts with the
  real node/session/workspace/agent names. Initial/reconnected/source-changed
  baselines do not replay historical state changes. Membership changes may show
  join/depart callouts. Events are sampled observations, not a lossless event log.
- Keep arrivals/departures eased over 1.2 seconds, with reversals starting at the
  current opacity. Retain at most three transient callouts, with stable placement.
- Regular viewer: show connection state, retained data and project affiliations;
  click a projected entity for its existing observations, click away to deselect.
  Include scoped project summary without changing current project ownership.
- Passive viewer: no controls or input interactions. Fullscreen and parent HWND
  preview use Orb. When the watch is unavailable, draw only `Daemon not available`;
  clear transient effects and never display retained fleet data or a fallback orb.
- Wrap footer/key and bound header/details areas. Projected hits use current
  positions and clip to the scene. Keep small previews readable with compact HUD.

## Implementation sequence

1. Add `--tree` parsing and tests without changing Windows launch semantics.
2. Port the accepted Orb WGSL, particle/line renderer, geometry, territories, key
   and footer. Strip gallery scenes and simulation controls. Add only bytemuck
   and glam; retain existing wgpu/egui versions and native shell.
3. Add a presentation adapter with stable scoped keys, parent-relative slots,
   bounded retained exits and current membership, real state/name capture and
   epoch-aware deltas. Reuse heartbeat tracking and fresh-count computation.
4. Budget rendered detail explicitly to fixed particle/line/entity limits, keep
   ancestors and all member nodes, report omitted detail, and keep totals complete.
   Slot/exit maps must be bounded over long-running churn; reuse fully freed slots.
5. Add Orb UI and projected observations, then integrate its GPU pass before egui
   in the shared renderer. Keep texture delta handling and per-surface depth resize.
   Preserve shared GPU, DX12 HWND presentation and screensaver lifecycle behavior.
6. Test adapter identities and incarnation changes, unknown/idle/stale states,
   baseline and delta events, heartbeat resets, churn/budget limits, counts and
   responsive HUD. Test launch defaults and legacy flag in both binaries.
7. Run format, strict all-target clippy, Rust tests (screensaver feature), release
   builds, Go/Rust wire contract and Linux native offline/live fixture checks.
   Check Windows compilation when a cross toolchain is available. Native Windows,
   macOS, preview/DPI/multi-monitor acceptance requires actual platform evidence.
8. Adversarially review the final diff, fix findings, update docs/ai-core memory,
   push and create a PR. Leave it unmerged.

## Adversarial plan review

- **Synthetic assumptions:** do not configure the rectangular prototype topology
  from live counts. It invents children, collapses scoped identities and forces
  every agent into one of three states. Replace only the presentation adapter.
- **Identity and churn:** sorted labels and unbounded monotonically assigned slots
  would cause movement, leakage or anchor indexing failures. Use scoped identities,
  free slots only after exit animations, bound retained detail and clear on source
  changes. Default/named observations remain separate as the current contract says.
- **Capacity:** protocol bounds are much larger than prototype bounds. Debug-only
  assertions are insufficient. Enforce budgets before upload, reserve effect
  capacity, expose sampling and never derive totals from sampled geometry.
- **Replay/claims:** reconnects, regressions, duplicates and snapshot baselines are
  not observed work. Reuse receipt tracking; only fresh live state deltas generate
  attention callouts. Idle/unknown and stale data need explicit representation.
- **Screensaver isolation:** the regular viewer's retained observations must not
  leak into passive presentation. Gate Orb GPU drawing and all HUD on live watch,
  including small preview and loss/reconnect. Keep input dismissal outside Orb.
- **Rendering:** per-window surface formats, DPI, minimized/lost surfaces and
  depth targets must stay aligned. Acquire the frame before egui deltas; load the
  Orb output when drawing egui. A single watch feeds all screensaver surfaces.
- **Compatibility:** legacy tree, headless wire checks, packaging and observer
  protocol remain usable. No daemon registration/auth/listener changes are needed.

Plan reviewed against current source; the issues above are incorporated before
implementation. Validation receipts and final review findings follow below.

## Final implementation review

The production client, projection, heartbeat tracker, summary and observer wire
contract remain unchanged. Orb replaces the synthetic adapter with scoped-key
slots, reusable only after exits finish. It admits at most 4,000 current and 8,000
retained entity marks, with 20,000 current/40,000 retained particle and line cost
budgets. Remaining fixed GPU capacity is reserved for decorative rings, the
coordinator and up to 128 simultaneous receipt effects. Rendered-detail omission
is explicit and does not change footer or Fleet pulse totals. Session counts are
scoped projected contexts, including an unknown default context; workspace totals
use reported inventory, excluding unresolved placeholders.

Review fixes incorporated:

- Retry capacity-limited arrivals after exits free slots without requiring a new
  snapshot. Keep node hubs ahead of child detail in admission order.
- Bound slot churn and reuse per-parent slots after exits; use per-reconcile slot
  cursors and clone name paths only for admitted entities to avoid repeated scans
  and amplified allocations on large inventories.
- Share callout duration/fade constants so opacity and expiry cannot diverge.
- Reset selection/effects on observation-source changes. Baseline reconnects and
  stale state comparisons cannot generate replayed attention events.
- Keep idle/unknown agents neutral and preserve true all-state totals.
- Keep small live previews predominantly GPU scene with compact real totals;
  disconnected passive views contain only the unavailable message.
- Reserve measured footer height for the expanded key and bounded callouts.
- Validate hard GPU capacity before upload in release builds as well as budgeting
  admitted detail and testing worst-case geometry.
- Preserve frame acquisition before egui texture deltas, per-surface depth resize,
  shared GPU and the existing Windows HWND/input/lifecycle behavior.
- Ship the imported MIT notice in viewer/screensaver archives alongside the one
  executable. Archive names/checksums and the separate Go CLI packaging stay intact.

## Validation receipts (Linux)

- Formatting and strict all-target clippy with the screensaver feature passed.
- Full Rust suite passed: 12 library tests, 40 tests in each launcher and six
  observer integration tests (98 executions, 58 distinct checks).
- Optimized regular and screensaver-feature binaries built on Linux. This checks
  shared source; Windows-specific presentation still needs native acceptance.
- The built viewer decoded the 5 MiB Go observer wire fixture successfully.
- Native Linux Orb rendering was inspected through the existing Go observer bridge:
  verified coordinator, online/offline nodes, default/named contexts, workspaces,
  agent states, full Fleet pulse cards, expanded key, real footer and receipt pulses.
  The opt-in window smoke passed at its inspection deadline.
- Pointer event tests verify selection and blank-space deselection. Compact live
  preview, unavailable passive view, retained ordinary view, source/incarnation
  changes, churn/retry and dense inventories with 128 concurrent receipts passed.
- Five packaging regressions passed; the Linux ZIP and checksum were generated.
  Archives carry the imported MIT notice without adding a runtime dependency.

Windows/macOS native graphics, Windows preview/DPI/multi-monitor interaction and
maximum-density performance profiling remain platform acceptance work. PR CI
builds/tests the three native platforms; CI compilation is separate from graphical
acceptance. No daemon upgrade, enrollment, listener configuration or OS screensaver
activation was performed. The PR is intentionally left unmerged.

## DPI follow-up

The user confirmed that the native Windows viewer works, then reported agent
markers appearing too small on a scaled display. The marker scale was capped in
physical pixels, while egui geometry and text were in logical display points.
Branch lines used native one-physical-pixel LineList rasterization.

The physical viewport now carries the current frame's pixels-per-point value.
Marker-size limits are applied to logical viewport dimensions, then converted to
physical pixels exactly once. This covers every particle glyph, halo and receipt
effect. The same camera projection still places the world geometry and logical
selection/callout anchors. Branches, including workspace diamonds and decorative
rings, now use instanced screen-space triangle quads with one-logical-point
width and a physical-pixel antialias fringe. Existing per-window egui scale-factor
events provide the scale for viewer, preview and fullscreen surfaces each frame.

Adversarial checks cover the small-preview lower size limit and large-window
upper size limit; fractional scaling and viewport/scissor clipping; CPU/GPU
projection agreement; every agent state; and the unchanged fixed line-buffer
budget. Each segment reuses two adjacent geometry vertices as one instance, with
an explicit even-length check and no additional CPU geometry allocation. A
view-aligned/zero-length segment cannot introduce a normalization NaN.

The native GPU readback regression passed on Linux at 100%, 125%, 150%, 200%,
300% and 400%: normalized marker area stays within 6% of the 100% raster sample,
and branch coverage stays within 5% of one logical point. The portable camera
regression checks marker size and projected anchors from tiny previews through
large fullscreen views at all six scales. Actual Windows DPI and mixed-monitor
interaction need a user retest; cross-platform rendering uses the shared path.

## Native screenshot legibility follow-up

Inspection of the real local observation stream in both a large window and a
smaller tiled window confirmed that DPI consistency alone preserved undersized
glyphs. Agent cores were close to the decorative field's dots, and idle/completed
brightness also reduced their radius. The user confirmed that readability comes
from size and requested that session rings retain their original surface alignment.

Agent core size is now 5 reference points, with state breathing affecting color
intensity only; membership/freshness fading still applies. The logical marker
scale floor is 0.9 instead of 0.25, so small windows and previews keep recognizable
symbols while the world-space constellation continues to fit the available area.
Compact agent halos preserve ownership gaps. Session hubs have larger/brighter
24-dot rings tangent to the sphere at each session's position, with their normal
pointing outward from the sphere center. They rotate with the globe and naturally
foreshorten toward its edges, preserving the impression of surface attachment. The
legend uses the same glyph geometry with adjusted magnification. Background dots
and halos are smaller and dimmer to give semantic marks priority.

Review/regressions cover equal agent core sizes across all states and breathing
phases, a readable preview-size floor, and enlarged surface-aligned session rings over
multiple clusters and rotations. Geometry counts, slot budgets, scoped data,
freshness rules, DPI conversion, pulse behavior and shader layout are unchanged.
Native screenshots were inspected at large and small window sizes; snapshots of
the live local fleet are kept out of repository/memory artifacts.
