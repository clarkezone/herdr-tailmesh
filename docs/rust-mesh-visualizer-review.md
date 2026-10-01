# Rust mesh visualizer implementation review

The first cut implements the reviewed ai-core functional/implementation specs.
It uses the managed daemon's existing authenticated connection and a separate
observation-only loopback service. It introduces no Rust Tailscale enrollment
or Go interop runtime. Build and operating instructions are in
[the viewer README](../visualizer/README.md).

| Challenged behavior | Implementation and evidence |
| --- | --- |
| Unauthenticated local control exposure | The observer registers only `GetInfo` and `WatchNodes`. Go tests reject Fleet reads/mutations and reflection on this port. |
| Caller credentials or upstream diagnostic leakage | Outgoing metadata starts empty; errors preserve bounded categories without upstream text. Tested through a real gRPC hop. |
| Optional UI failure stopping mesh | Listener failures stay outside the role errgroup. An occupied-port integration test verifies the daemon reaches ready and private IPC remains usable. |
| Unsafe Windows process matching | The port travels through a replaced numeric environment value. `managed-run --state-dir <absolute-dir>` is unchanged. Invalid flags fail before onboarding effects; running-daemon overrides are rejected. |
| Unbounded subscriptions or payloads | Four observer watches, sixteen TCP connections, four-minute upstream watch deadline, 8 MiB snapshot limit. Capacity, cancellation, deadline and size tests cover the bridge. |
| Rust's ordinary 4 MiB message limit | The generated client permits 8 MiB. A real Go observer bridge sends a 5 MiB snapshot to the built Rust viewer's headless check. |
| Quiet watches appearing offline | No inactivity timeout after the first snapshot. A simulated 35-second quiet stream remains live; node/session receipt age remains separate. |
| Missing first snapshot | Bounded connect/handshake and first-snapshot deadlines, with cancellation and retry. Tested with a silent observer. |
| Lost or malformed replacements | Retain the previous scene as last-known; retry, then replace the entire scene. Tests cover invalid duplicates and lease-expiry replacement. |
| Native ID collisions or false project placement | Composite node/session/incarnation/workspace/tab/agent keys, unresolved branches, and workspace-owned project placement. Projection tests cover overlapping sessions and affiliation conflicts. |
| Recent timestamps falsely making unavailable sessions live | Herdr/session readiness also controls freshness; old agent records stay last-known. Receipt timestamps alone never override unavailable state. |
| Large snapshot stalls | RPC decode and projection run on a worker; mailbox and wake events are coalesced. Indexed pane/tab joins avoid quadratic searches. Paint work is clipped to the viewport. |
| Untrusted display text | Strip control characters, truncate canvas labels at 160 Unicode characters, allow selectable detail values up to 4,096 characters, and clip rendering. Normal long paths remain inspectable; oversized text is bounded. |
| Generated `Connect` name collision | Disable tonic's transport convenience constructors; create clients from explicit channels. The canonical proto remains shared with Go. |
| Release archive regressions | Viewer packaging creates a separate one-executable ZIP and checksum in `dist/visualizer`; the Go release script is unchanged. |
| Wrong project abstraction | Ownership is node → session → workspace → agent. Workspace affiliation is displayed locally; a separate summary counts exact declared IDs, scoped workspace observations, and distinct nodes. No Git-equivalence claim or backend/schema change. |
| Interrupted animation or perpetual redraw | Retargetable 400 ms geometry/opacity transitions start from their current sample, ignore unchanged targets, and stop at rest. Pane allocation animates; row connectors/hit areas use current geometry. Tests advance a synthetic clock instead of sleeping. |
| Ghost selection and retained render data | Fading exits have no entity hit target and are pruned after the transition. Collapse/reopen and empty replacement tests cover interruption and cleanup. Removed selected identities are reconciled before pane drawing. |
| Window shrinking during animation | Target text wraps within the new width; interpolated row geometry is immediately contained/clipped to the viewport. The outer margin and blank-click selection behavior remain covered by interaction tests. |

Local checks cover Go tests/vet, affected-package race checks, Rust format/lint
and tests, the Go-to-Rust wire contract, and an optimized Linux build. A Linux
native GPU smoke rendered default/named sessions, working/blocked agents and an
offline node from a Go bridge fixture. The fixture uses no Tailscale identity.

Windows/macOS/Linux CI jobs build/test and package the viewer separately.
Those jobs are configured in source; their remote execution has not been
claimed here. Native Windows/macOS acceptance, comprehensive graphical
interaction/DPI checks, and attachment to upgraded live managed installations
on the established target machines remain outstanding. The first cut is built;
the full cross-platform live acceptance matrix is not yet certified.

## Pre-merge adversarial review (2026-10-01)

The review covered the complete change against main: observer method registration,
metadata/error handling, optional listener lifecycle and shutdown, bounded RPCs,
per-launch overrides and Windows argv matching, Rust cancellation/reconnection,
projection joins/identity, UI transitions/hit targets, native graphics cleanup,
CI and packaging. The following findings were fixed before merge:

- Observation scrolling used the pane's full height after adding heading/name/status
  rows, leaving the bottom of its viewport clipped and details potentially
  unreachable. Limit the scroll viewport to actual remaining height. A regression
  test verifies it fits below those rows.
- Surface timeout/occlusion/loss could return after egui produced texture updates,
  leaving texture frees unapplied (and triggering its debug drop assertion).
  Acquire the surface before running egui so those early returns produce no deltas.
- The wire message-size limit alone did not bound amplification when unusually
  long identifiers were cloned into every scoped descendant key. Validate the
  existing Go node/session/entity count and identifier-length limits before
  projection. Regression tests reject excessive counts and oversized identities;
  the 5 MiB display-name wire fixture remains accepted and truncated for display.
- Longer animations made fixed frame-count waits unreliable. Settle interaction
  tests relative to the shared duration and verify interrupted retargeting,
  monotonic/symmetric easing, moving hit targets, fade cleanup and idle completion.

Geometry, opacity and pane allocation now share 400 ms quintic ease-in/ease-out
with zero velocity and acceleration at the endpoints. Repeated identical targets
still do not restart a transition. No project binding architecture, coordinator
root, remote exposure, destination dialog, heartbeat pulse or Fleet pulse cards
are implemented in this PR; the latter features have separate ai-core requirements.

Local verification: full Go tests/vet, affected-package race tests, dashboard model
checks, Rust format/clippy and twenty tests, release build/packaging, and the real
Go-to-Rust observer wire contract. Cross-platform CI must pass before merge.
Native Windows/macOS graphical acceptance remains a separate follow-up.
