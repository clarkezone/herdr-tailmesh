# Ai-core memory update record

The ai-core connector rejected writes with:
`MCP tool call requires approval, but approval policy is never`.
After the session permissions changed, MCP writes succeeded. The factual update
below was synchronized to current-state, decisions, implementation specification
and adversarial review in the memory vault on 2026-10-01.

## Project current-state

The first-cut source implementation now contains a narrow LocalObserver service
in the managed Go daemon and a separate Rust winit/wgpu/egui text-tree viewer
under `visualizer/`. Port overrides are per launch; the viewer has bounded RPC
streams, scoped identities, unresolved/project-conflict projection and truthful
stale/reconnect handling. No installed-daemon upgrade or new Tailscale enrollment
was performed.

Local validation passed the full Go tests/vet, affected-package race checks,
Rust format/lint and nine Rust tests. A built Rust client decoded a 5 MiB snapshot
through a real Go observer bridge. A Linux native GPU smoke displayed default
and named sessions, working/blocked agents and an offline node from a fixture.
An optimized Linux viewer was built, with separate packaging/checksums.

Windows/macOS/Linux CI jobs are configured in source. Remote CI execution,
native Windows/macOS, comprehensive interaction/DPI behavior and attachment to
upgraded live managed installations remain outstanding. The first cut is built;
the full cross-platform live acceptance matrix remains open.

## Project decisions

Treat stream health separately from observation freshness. An unchanged healthy
watch remains live while node/session receipt times and readiness determine
freshness. Bound the first-snapshot wait and renew stream leases. Invalid
replacement snapshots retain the last valid scene as last known.

## Implementation specification and adversarial review

Change the implementation status to “First-cut source implementation built;
full platform acceptance pending.” Retain the existing functional acceptance
requirements. Add the implementation validation evidence above and reference
repository `visualizer/README.md` and `docs/rust-mesh-visualizer-review.md`.

The imported native shell is based on wgputests commit
`56551e298420764e43d6851efc9a2702273af0e1`. Implemented bounds: four observer
watches, sixteen observer TCP connections, four-minute upstream watch,
270-second Rust watch request, 8 MiB snapshot, 160-character canvas labels and
4,096-character selectable details. Larger detail values are visibly truncated.

The implementation review fixed tonic's generated Connect constructor collision
by disabling transport convenience constructors, handled its separate
oversized-message error code, prevented unavailable sessions from appearing
fresh solely because of receipt timestamps, replaced quadratic pane/tab joins,
made longer details selectable and added the chosen port to failure diagnostics.

Design review and first-cut implementation review are complete. Do not claim
that the remaining full live platform acceptance has run.

## Responsive layout follow-up (2026-10-01)

The user reports the viewer working. Children now stack below parents with
width/depth-dependent rightward indentation, wrapped labels/status and measured
row heights. The canvas scrolls vertically; narrow windows place the collapsible
observation pane above it. The sidebar explicitly lays details out vertically.
Eleven Rust tests and format/clippy checks passed, including deep trees, long
Unicode/unbroken labels, canvas widths 120–2000 and whole-view resizes 280–1200
with selection retained. A Linux GPU fixture smoke displayed the updated tree.
The optimized Linux viewer and standalone archive/checksum were rebuilt and
verified. The change requires only a viewer update. Current-state, decisions and
implementation specification in ai-core were updated with these facts.

The selection follow-up adds a 16-point outer margin, hides observations when
nothing is selected, and deselects on blank canvas/margin clicks. Real pointer
tests cover pane appearance, detail interaction, branch collapse, blank row and
below-tree clicks, width/height recovery, and selected-entity removal. All thirteen
Rust tests and format/clippy checks passed; the native Linux window displayed the
padded tree with observations hidden. Immediate egui redraw requests now reach
winit. The optimized Linux viewer/package were rebuilt and verified. Current-state
and decisions record the behavior; layout animation remains a proposed follow-up.

## Animation and workspace affiliation (2026-10-01)

The next viewer-only change implements retargetable 250 ms geometry/opacity
transitions for snapshots, expand/collapse, resize and observation-pane layout.
Moving rows, connectors and hit targets share geometry; disappearing rows fade
without receiving clicks and are pruned. Text reflows for the destination width
and stays clipped during motion. Scrolling remains direct.

The ownership tree is now node → session → workspace → agent. Each workspace
displays its existing project affiliation; a separate project summary counts
scoped workspace observations and distinct nodes by exact declared project ID.
The snapshot cannot prove Git equivalence or aliases between configured-default
and named session contexts, so counts remain explicitly observations. No Go,
protobuf, persistence, binding or observer-exposure changes were made.

Seventeen Rust tests, format/clippy, Go-to-Rust wire validation and a Linux GPU
fixture smoke passed. The optimized Linux viewer and ZIP/checksum were built
and verified. Native Windows/macOS acceptance remains outstanding. Ai-core
current-state, decisions, functional/implementation specifications and review
were updated. Bulleted future requirements were created at
`projects/herdr-tailmesh/Functional Specs/Revised Project Binding Requirements.md`.

The user's subsequent proposals for role-dependent observation exposure,
CLI/GUI destination editing, a coordinator root and heartbeat pulse animation
are feedback-only follow-ups. They have not been implemented. The service still
runs on every managed role and listens only on IPv4 loopback. The existing
snapshot's `last_seen` timestamp supports newly observed heartbeat pulses;
the stream remains replacement snapshots rather than a heartbeat event log.

## PR preparation and follow-up requirements (2026-10-01)

Row geometry, opacity and pane allocation now use a shared 400 ms quintic
ease-in/ease-out curve. The pre-merge adversarial review fixed observation
scroll-height clipping, texture-delta cleanup on skipped/lost GPU frames, and
scoped-key memory amplification from oversized identities. Existing Go
inventory/key bounds are now checked before Rust projection. Twenty Rust tests,
format/clippy, full Go tests/vet, affected-package race checks and 56 dashboard
model tests passed. Release binaries are built independently of installed mesh
processes and enrolled state.

Four bulleted follow-up documents are saved under ai-core
`projects/herdr-tailmesh/Functional Specs/`: Observation Exposure Requirements,
Visualizer Connection Editing Requirements, Coordinator Root and Heartbeat
Visualization Requirements, and Fleet Pulse Panel Requirements. They specify
future behavior and are not implemented by the current PR. The separate
Revised Project Binding Requirements also remain future data-model work.
