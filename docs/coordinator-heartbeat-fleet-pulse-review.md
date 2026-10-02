# Coordinator heartbeat and Fleet pulse implementation review

Functional and implementation specifications for both features, plus their
adversarial specification review, are saved in the herdr-tailmesh ai-core project.
The user's observation-contract comment is incorporated: coordinator metadata is
an additive `ObserverInfo.coordinator` property on the existing `GetInfo` RPC.
No new service, sidecar, remote listener or enrollment is introduced.

## Reviewed behavior and fixes

- The control root is separate from execution branches and contributes to no
  node, workspace, agent or project total. Existing execution keys stay intact.
  Root selection/collapse is tested; confirmed source changes reset UI state.
- `GetInfo` borrows the authenticated upstream connection and copies only
  verified bounded identity/version properties. Empty outgoing metadata, a
  one-second deadline, 64 KiB response limit and four-call gate prevent
  credential forwarding and unbounded upstream amplification. Local identity
  remains available when upstream identity is absent or unavailable.
- A pending handshake cannot relabel an older retained scene. Coordinator data
  belongs to each accepted scene; an integration test checks that boundary.
- A watch epoch resets heartbeat baselines even when latest-value coalescing
  hides the intermediate disconnected state. Initial/new/reconnected nodes,
  duplicates, backwards timestamps and invalid/future receipts cannot replay
  activity. Strict seconds/nanoseconds advances drive one bounded pulse per node.
- Pulse paths use the same contained animated anchors as ordinary connectors.
  Collapsed/offscreen paths use visible ancestor/legend cues. Removed/offline
  nodes and disconnected streams clear effects; timed effects settle to idle.
- Final PR review fixed last-seen ages crossing a wall-clock second before a
  full elapsed second. Age display now uses complete timestamp differences;
  boundary tests also preserve explicit future-clock states.
- Counts come from actual validated records, excluding orphan workspace
  placeholders and aggregate branch statuses. Fresh contexts and retained raw
  counts remain separate; default/named aliases are scoped observations.
  Receipt precision and the dashboard's 30-second age/skew boundaries are tested.
  Canonical missing session receipts remain unknown: the dashboard emits explicit
  JSON null, so its legacy omitted-field discovery fallback must not be applied
  to protobuf snapshots. Independent coordinator/node receipt invariants agree.
- Live counts never interpolate fictional integers. Waiting/disconnected cards
  show em dashes, retained notes are explicit, and valid live empty fleets show
  zero. A quiet stream stays live while inventory ages out independently.
- Short-window review found that bounding cards alone left wrapped connection,
  legend and project text capable of consuming the tree viewport. The overview
  now has its own bounded scroll region. A second regression exposed egui's
  default 64-point minimum scroll viewport during pane animation; the tree now
  uses its actual remaining height. Full-window tests cover this at 280×260,
  480×320, 1200×320 and 280×600, preserving usable tree space.
- Card reflow and tree-space changes use 400 ms quintic transitions, with
  destination text wrapping, immediate clipping on shrink and unchanged-target
  settling. Heartbeat travel uses a separate 3 second effect and never expands
  branches or changes selection.
- Native inspection caught half-clipped card values/notes. The panel now fits a
  complete row when space permits, with six readable columns at medium widths;
  the compact purple heartbeat legend stays outside the scrolling overview.
  Coordinator connection status is distinct from execution inventory freshness.
  Info capacity failures are retryable rather than false API incompatibility.

## Validation and acceptance boundary

Local Rust format/clippy and 34 unit/integration tests pass. Full Go source
tests/vet and focused observer race checks pass. Optimized Go/Rust binaries,
the updated Go-to-Rust coordinator/5 MiB contract, and one-executable viewer ZIP
contents/permissions/CRC/SHA256 were verified. Native Linux fixture rendering
shows all six cards, default/named contexts, the coordinator root, last-seen
ages and upward pulse motion. The inspection ended at its two-minute deadline;
clean-close behavior was not certified by that fixture. Remote cross-platform CI and native
Windows/macOS graphical/DPI acceptance are not claimed for this follow-up.

A desktop notification during inspection belonged to a separate older debug
viewer. Its core/kernel record identifies a SIGSEGV in the Wayland clipboard
thread during `wl_proxy_destroy`; other threads were waiting and no OOM event
was recorded. The old executable had been replaced, so its exact Rust call
sequence could not be reliably recovered. The cause and recurrence remain
unconfirmed; this does not establish a feature-specific failure. No additional
core appeared during the new release fixture. The `diagnose-crash` skill was
used for read-only diagnosis; no desktop settings or installed daemons changed.

To obtain verified coordinator metadata, update and restart the Go daemon while
retaining its enrolled state, and update the viewer. An older observer remains
usable with an explicitly unknown root. Project binding/persistence, observation
exposure defaults and endpoint editing remain unchanged.

## Total agents follow-up

The panel now has seven cards, adding Total agents after Workspaces. Count all
reported agent records in eligible fresh contexts across the fleet, including
idle/unknown states and unresolved workspace placement. Retained counts use the
same records, with explicitly last-known notes on disconnect. Default/named
contexts retain their scoped-observation semantics. No daemon contract changes.

Review covered avoiding a sum of only working/blocked/done, excluding synthetic
branches from totals, counting each context once, and preserving freshness,
empty/unavailable semantics and short-window containment. Expanded fixtures
cover multiple workspaces, idle/unknown/orphan agents, default/named contexts,
offline retention, exact age thresholds, seven-card reflow and the displayed
live/retained total. Earlier native inspection above covered the original six
cards; it does not certify native graphics for the seventh card.
