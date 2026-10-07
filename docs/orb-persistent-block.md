# Persistent Block — Functional Specification
Status: reviewed for implementation (2026-10-07).
Source: [[projects/herdr-tailmesh/Visualization Backlog#Persistent blocked]]; current Orb implementation and user request. Viewer-only; preserve independent --tree and observer architecture.

## Behavior
- Each scoped blocked agent has a persistent ATTENTION REQUIRED callout, including blocked agents in the initial snapshot or reconnect baseline. Multiple blocked agents remain independently represented.
- Persistence follows observed current state, not a ten-second event timer. Working → blocked replaces the work card; blocked → working replaces attention with work. A fresh observed blocked → done/idle/unknown change becomes a ten-second actual-state notice. Reconnect/stale comparisons do not invent events.
- Retain disconnected/stale blocked cards explicitly as LAST KNOWN. Quiet stale data does not emit live attention lights. Node/agent removal and source replacement clear attention honestly; geometry sampling must not claim a departure.
- W affects working cards only. K/N remain independent. Retain bounded pagination and passive page rotation; “persistent” means retained until resolution, not every card visible simultaneously in an arbitrarily small window. GPU sampling does not discard cards: sampled leaves anchor to their node. At the 8,000-card history bound prioritize blocked attention, disclose capacity omissions, and keep full totals/node attention honest.
- Fresh blocked agents cause their owning node to breathe gently in amber and illuminate a restrained repeating circuit/ripple inside its surface cluster. This is an attention effect, separate from the existing three-second receipt heartbeat; no strobing, marker-size changes or fabricated heartbeats.

## Focus
- Every activity callout (working, blocked, completed and other timed states), and a selected node/descendant observation, offers a subtle Focus checkbox in interactive mode.
- Focus belongs to one scoped mesh node. Checking another node transfers focus. Checking any card of the focused node reflects the same checked state; unchecking, Escape or Return to fleet releases it.
- Freeze orbital/spin motion, bring the node's surface cluster to the front, and smoothly zoom/frame that cluster. Use continuous eased transitions, including mid-animation retargeting, resize, and return to normal rotation. Data, attention lights, heartbeat effects and callout timers continue.
- Show readable on-scene names when projected label space permits and a scrollable full node → session → workspace → agent hierarchy with real names and state/freshness. This guarantees access to every name even in dense clusters or sampled geometry, without unreadable overlapping labels.
- Focus survives the originating timed callout's expiry and W hiding working callouts. A visible focused-node panel and Escape always permit release. Release on node removal, coordinator/source replacement or absence of a scene; never transfer focus through a recycled presentation slot.
- Passive screensaver/preview retains persistent attention and lighting, without interactive focus controls. Unavailable passive views still show only Daemon not available.
- Logical display coordinates and shared CPU/GPU camera transforms must preserve DPI sizing, hit testing and leaders on Windows/macOS/Linux.

## Acceptance
- Startup/reconnect blocked snapshots; concurrent blocked and working agents; >10-second persistence; every resolution state; stale/disconnect, source change/removal, scoped duplicate names and rendering-budget omission.
- W cannot hide blocked cards. Overflow does not expire attention.
- Actual checkbox and Escape input; single-node focus, transfer/reversal, timed-card expiry, removal, labels/roster and narrow resize.
- Front-facing orientation, paused motion, continuing effect clocks, finite bounded framing and matching CPU projection/GPU camera at fractional DPI.
- Format, strict screensaver-feature clippy, Rust suite, optimized launchers, adversarial implementation review and platform CI. Native graphical evidence is reported separately.

# Persistent Block — Implementation Specification
Status: reviewed for implementation (2026-10-07).
Functional contract: [[projects/herdr-tailmesh/Functional Specs/Persistent Block Functional Spec]].

## Model
- Extend Activity with its observed state; persistence includes Working and Blocked. Refresh names without restarting an unchanged persistent card. A different state receives a new serial/reveal.
- Reuse scoped keys, reconciliation and freshness. Retain persistent activity independently of GPU geometry, anchoring sampled leaves to their scoped node. Bound history at 8,000, evict timed history then working cards before blocked attention, disclose capacity omissions, and retry admission after expired history frees space. Baselines establish persistent current state without inventing transitions; only actual disappearance can emit removal.
- Filter working visibility by activity state, not by persistence. Timed notices keep first-page priority; persistent blocked attention precedes persistent working cards.

## Focus/camera
- Add a viewer presentation-only camera controller and sampled pose. Store requested node identity in Panels; resolve identity each frame against current scene and presentation IDs.
- Use a frozen orbital time and independent live effect clock while focused. Ease quaternion orientation, model translation and zoom over 1.5 seconds; sample current pose before retargeting. On release resume rotation without a jump.
- Derive focus framing from admitted descendant geometry; fit finite bounds to viewport and available central space, preserving logical marker sizes. Bound zoom and handle tiny/portrait windows.
- Apply the same sampled pose to GPU uniforms, CPU projection, picking, selection rings, callout leaders and projected labels. Keep legacy camera helper for existing unfocused mathematical tests.
- Add Focus to activity/observation cards and a dedicated focused-node hierarchy panel with Return to fleet. Reserve its rectangle in callout packing. Virtualize the complete node hierarchy list; bound projected labels and avoid collisions, disclosing incomplete projected labels through the full list.

## Lighting
- Cache unique blocked inventory receipts per scoped node from the complete observation and age them each frame; require fresh owning-node status too, independently of sampled agent geometry. Reuse palette/glyph primitives for slow amber node breathing and bounded cluster circuit/ripple lights; do not alter receipt tracker.
- Keep GPU particle/line limits including maximum simultaneous heartbeat and attention effects; suppress live effects for stale/offline observations.

## Verification/review
- Add behavioral regressions for persistence transitions, visibility independence, baseline/staleness/removal, checkbox interaction, focus lifecycle and projection agreement.
- Adversarial review must challenge W accidentally hiding blocked cards, persistence mutation without state comparison, disconnected fake attention, recycled focus IDs, camera/leader mismatch, rotation jumps, unavailable focus exit, label overflow and GPU bounds.
- Complete focused local verification and native Linux rendering where available; push a dedicated PR, inspect final platform CI and merge following the requested prior feature workflow. Record unresolved native Windows/macOS acceptance rather than claim it from builds.

# Persistent Block — Adversarial Review
2026-10-07. Reviewed against the functional spec, implementation spec and prior Orb source.

## Spec review and fixes
- Persistence was previously synonymous with Working. Adding Blocked without explicit state would make W hide attention, and Working → Blocked would only refresh the old heading. Store observed activity state, compare state before reusing an event, and filter W by Working only.
- Startup blocked is current state, not a replayed event. Create persistent cards from baseline snapshots while keeping transient events suppressed. Stale/disconnected cards are last known; fresh attention effects require both agent and owning node freshness.
- Completed cards expire after ten seconds, so checkbox-only focus exit would strand the camera. Focus belongs to a scoped node and has its own Return to fleet/Escape exit independent of card lifetime and visibility.
- A rotating GPU model with unchanged CPU projection would break picking/leaders. Sample one pose used by all render/projection consumers, freeze orbital positions separately from continuing effect clocks.
- Zoom translating the sphere would invalidate its atmospheric shader's origin assumption. Compute rear attenuation and ray intersection relative to the translated sphere center.
- Dense clusters cannot display every label without collisions. Bound measured/projected labels and provide a virtualized full scoped hierarchy list, including sampled geometry.
- HUD/observation/callout reservations and compact views must bound the focus panel; tiny previews retain keyboard exit and an enlarge-view hint.
- Focus must resolve current node key each frame, release on removal/source reset, and never follow a recycled slot. Return/transfer/reversal must sample displayed camera state.

## Implementation verification
Behavioral tests cover persistent startup blocking/resolution, baseline honesty, stale/offline attention, W independence, real checkbox input, scoped removal, smooth focus transfer/release, paused motion with live effect clocks, fractional-DPI projection agreement, resize framing and combined attention/heartbeat GPU budgets. Final local validation is below; platform CI will be recorded after completion.

## Final code review and local validation
- Dense-fleet testing exposed attention depending on geometry admission. Fixed with complete-observation receipt caching, independent scoped activity retention and node anchors for sampled leaves. Timed history and working cards yield to blocked attention at the existing 8,000-card limit; capacity omissions remain explicit.
- Preserve freshness on each activity, so sampled leaves have truthful LAST KNOWN text. Fresh sampled resolutions use actual-state notices; baseline comparisons cannot invent stops.
- Bound projected text work to 256 measurements/128 placed labels, use full virtual hierarchy scrolling, and reserve the focus panel outside drifting HUD/callout bounds. Tiny focus panes avoid inverted child rectangles and retain Escape exit.
- Local format, strict screensaver-feature clippy and 90 distinct portable Rust tests passed (162 executions across both launchers; optional native GPU tests excluded). Optimized Linux launchers were built and the Go native observer fixture rendered persistent baseline blocked cards, amber attention, Focus zoom/labels/full hierarchy and W hiding working cards only. Both native smoke processes passed; one reached its inspection deadline and one closed normally. The screenshot helper's OCR detection was unreliable; graphical acceptance was established by inspecting captures, not its OCR result.
- Existing geometry budgets, receipt heartbeat tracker, source identity/freshness, passive unavailable behavior and independent --tree remain preserved. GPU/CPU focused projection agrees at fractional DPI in tests. Native Windows/macOS graphics, mixed-monitor DPI and screensaver acceptance remain manual; final platform CI is pending.

- Final adversarial review fixed the worst-case retained-exit line budget: sixteen attention circuit segments per node keep 32 scanner lamps while reserving space for 40,000 retained line vertices and 1,440 orbit vertices. A dense repeated-session-incarnation regression exercises attention, receipt effects and multiple concurrent exits.
