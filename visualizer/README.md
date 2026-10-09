# Herdr mesh visualizer

A standalone Rust viewer for the managed Herdr mesh daemon. **Orb is the default**
for both the interactive executable and Windows screensaver/preview. It presents
real coordinator → node → session → workspace → agent observations using the
accepted orbital geometry, depth cues, role colors, agent states and 3-second
receipt pulses. There is no simulation or debug lab. Idle/unknown agents use
neutral gray; stale/offline observations are dimmed.

Orb uses the full, centered viewport with a blank border. Two narrow floating
panels hold the visual key and the seven fleet totals in vertical lists. Their
backgrounds cover only their own bounds; they gently drift and connect to the
coordinator. The key appears once for the first minute after launch, then stays
hidden. **K** toggles it manually, overriding the startup timer indefinitely.
The separate counts panel starts visible and stays on until **F** toggles it.
When both are visible they stack without overlap. When either is alone it hugs
the bottom-left margin; showing the other smoothly moves it up before the new
panel unfolds beneath it. Held keys do not repeatedly toggle. Reconnecting does
not reset these preferences or replay the startup key.

**N**, **S**, and **W** independently toggle node, session, and workspace
callouts. Clicking any sphere object opens its typed card with the exact legend
glyph, scoped names, status and details. Click again, blank space, or its bracketed
× to close it. Bulk and clicked cards share one identity; closing a bulk card hides
that item until its class is toggled off/on or it is selected again. Typed × never
acknowledges historical outcomes. Node cards show herdr-mesh and session Herdr
versions; old coordinators report the mesh version as unknown. Rebuild the
coordinator to expose the additive NodeView metadata (existing node Hello already
reports it). Coordinator Focus pauses a fleet view with the root forward; descendant
Focus frames its node rose. Cards enter with a 200 ms visible-page stagger and
reposition over 400 ms with easing. New outcomes/attention take priority over bulk
cards; overflow remains pageable. Keyboard toggles are omitted in screensaver mode.

Panels and working-agent cards share a holographic reveal: a travelling leader
beam, expanding light rail, upward unfolding aperture, luminous scan edge,
circuit ticks and scan lines. The reverse effect hides them. Text and glyphs keep
their logical sizes throughout. Hidden cards/panels have no clickable footprint.
Tiny previews use abbreviated vertical counts and a bounded key hint when the
full visual key cannot fit. Activity pagination remains in the first callout,
independently of whether the key is showing.

Each observed working, blocked or completed agent has an independent persistent callout with
its real node/session/workspace/agent names, including initial/reconnect snapshots.
**A** toggles working callouts only; blocked and completed cards remain visible. State and
counts, selection and timed notices remain independent of A. Working → blocked
replaces the work card with persistent ATTENTION REQUIRED, and blocked → working
replaces attention with work. Each agent has one latest outcome card, independent of its current state:

- **Working → Done → Idle** retains **AGENT COMPLETED** until acknowledged.
- **Working → Idle**, without Done, retains **WORK STOPPED — IDLE** with neutral
  Idle color. Startup Idle without recorded history creates no stop.
- Starting work again keeps the previous unread outcome beside the live Working
  card. The next Done or direct Working → Idle replaces that outcome with a fresh
  instance. Old dismissal input cannot hide the replacement.
- Each card shows its outcome, current state and node/session/workspace/agent
  names. Counts and markers reflect current observations, including Idle after
  completion; history does not inflate Completed counts. A hides live Working
  cards only. Focus resolves to the actual agent, not the historical card.
- A bracketed **×** beside the Focus reticle acknowledges only that outcome
  instance and retracts it. The control glows on hover/keyboard focus; hover help
  identifies Dismiss. Received snapshots are tracked before UI coalescing, so
  intermediate Working or Done does not have to be drawn.
- Latest outcomes and dismissal survive viewer/screensaver restart, including
  startup while Idle, Working or no longer observed. Removed agents retain
  truthful missing-agent context; leaders appear only when an actual agent/node
  anchor is available. A new coordinator/session incarnation has a separate scope.

Cards retain sci-fi reveal/retract, first-page stop priority, paging and LAST
KNOWN freshness treatment. Other fresh observed transitions to unknown can show timed notices.
Historical outcomes are not
replaced by those notices. Verified same-source reconnect preserves observations;
unverified reconnect starts a baseline. The viewer cannot reconstruct work
completed entirely while closed or a Done → Done cycle with no intervening
observation/completion identifier. See [the spec and review](../docs/orb-persistent-idle-stops.md).

The latest-outcome journal is written immediately when an outcome is observed
and updated synchronously when × is clicked, without waiting for app exit.
It is `herdr-mesh-visualizer/dismissals-<port>.outcomes.bin` under `%LOCALAPPDATA%`
(Windows), `~/Library/Application Support` (macOS), or `$XDG_STATE_HOME` /
`~/.local/state` (Linux). It contains verified coordinator/full scoped agent
identity, instance token, outcome kind, hierarchy display names, observation
stamp and dismissal. It contains no mesh credentials. The older
`dismissals-<port>.bin` acknowledgement file is retained for migration; its format
is unchanged. Close viewers and delete both files to reset local outcome history.
`--tree` does not read or write this state.

Independent writers merge under a bounded file lock with conditional instance
replacement, atomic file replacement and explicit errors/retries. The journal
holds up to 8,000 outcomes across sources and 16 MiB. At capacity, only dismissed
records outside current Done may be recycled; unread history is preserved and
further admissions report an error. The independent 8,000-card presentation bound
prioritizes Blocked/Working and discloses omitted cards; fleet totals remain
complete. Unverified source or unavailable storage allows session-only history.
Native Windows/macOS graphical/restart acceptance remains manual.

Orb automatically records completion diagnostics, including Windows screensaver
and preview launches. No flag or `RUST_LOG` setting is needed. Logs are JSON Lines
under the same user state root as acknowledgements, in
`herdr-mesh-visualizer/logs/`:

| Platform | Default directory |
| --- | --- |
| Windows | `%LOCALAPPDATA%\herdr-mesh-visualizer\logs` |
| macOS | `~/Library/Application Support/herdr-mesh-visualizer/logs` |
| Linux | `$XDG_STATE_HOME/herdr-mesh-visualizer/logs`, otherwise `~/.local/state/herdr-mesh-visualizer/logs` |

The current file is `completion-<port>-<slot>.jsonl`; `.1.jsonl` and `.2.jsonl`
are its older rotations. Each file is capped at 2 MiB. Eight independently locked
slots support concurrent processes without sharing a writer or deleting an active
process's log. A launch reuses the first free slot and appends to its previous log;
the run ID separates launches. The terminal prints the actual current path.
For a missing banner, retain the matching port's current and rotated files soon
after the event, together with the approximate time and agent name.

Records identify the build commit/dirty state, run, timestamp, sequence, port,
coordinator and scoped agent/completion IDs. Completion IDs use decimal strings to preserve
all 64 bits in JSON tools. Records trace accepted snapshot counts,
received agent state changes, completion creation/retirement, acknowledgement
input/save/restore/retirement, model activity changes and per-renderer flyout
placement. `completion_flyout` explains `drawn`, `revealing`, `off_page`,
`acknowledged`, `acknowledgement_retracting`, `activity_missing`,
`activity_episode_mismatch`, `candidate_excluded`, `no_layout_space` or
`passive_unavailable`. A `drawn` record means the CPU UI submitted the fully
revealed card; it is not a GPU/display readback guarantee. Counts and paging
remain independent of acknowledgement.

The same files record Idle work stops with `idle_stop_created`,
`idle_stop_retired`, `idle_stop_priority`, `idle_stop_flyout` and
`idle_stop_flyout_retired`. Raw `completion_created`/`completion_retired` and
`idle_stop_created`/`idle_stop_retired` describe current observed-state episodes,
not retained outcome lifetime. `outcome_available` records the latest retained
kind/token, dismissed state and superseded previous token/kind. UI records use
`key` for the real agent and `card_key` for independent presentation identity.
All episode tokens are decimal strings.

For a dismissal that returns after relaunch, retain logs from both launches.
`dismiss_input` identifies the UI action; `outcome_ack_requested`,
`outcome_ack_saved` and `outcome_ack_applied` report receiver revalidation and
synchronous journal durability. `persisted: false` means session-only input or a
storage error, not a successful save. `outcome_journal_ready` records published
pending/dismissed counts and durability, including the initial publication;
`outcome_journal_error` and `outcome_journal_recovered` explain failures/retries.
`outcome_ack_rejected_stale` identifies input for a superseded instance;
`outcome_ack_imported` reports a concurrent dismissal. Legacy `ack_restore_*`
records describe migration from the older acknowledgement file and do not retire
the independently retained latest outcome. Compare build, port, source, full key
and episode across the two runs. A changed identity is evidence to investigate,
not proof of user dismissal or an upstream fault.

State and visibility are logged only when they change; receipt counts are logged
per accepted snapshot, never per animation frame. A bounded background queue
keeps file I/O out of rendering and observation callbacks. Overflow/oversized
records are disclosed through sequence gaps, `dropped_before` and a shutdown
`diagnostic_loss` total; missing records during reported loss cannot prove an
upstream fault. Storage failures go to stderr and do not stop observation.
After an unclean exit the final record may be partial. A new launch separates
that tail before appending; `launch.data.previous_tail_separated` reports it.
Preserve the partial evidence and skip the malformed tail when parsing older
records. Logs contain identity/name/state metadata, not full snapshots, prompts, workspace
directory details, credentials or RPC bodies. `--tree`, `--check`, help and
screensaver configuration do not start this completion recorder.

Fresh blocked agents cause a gentle amber node beacon and a repeating circuit/ripple
inside their surface cluster. This uses full observations even when geometry is
sampled, ages with inventory freshness, and remains separate from receipt heartbeat
pulses. Sampled agent callouts anchor to their owning node. Callout history is capped
at 8,000, with blocked attention ahead of working, then completed cards and old timed history; capacity
omissions are disclosed independently of K/F/A, while totals remain complete.

Every activity callout and typed object card offers a compact
illuminated reticle **Focus** toggle.
Checking it pauses orbital motion and eases that node's surface cluster forward
with zoom over 1.5 seconds; focus framing has a further 20% magnification boost. Another node transfers focus; uncheck, **Escape**, or
**Return to fleet** restores the rotating overview without a camera jump. Focus
survives callout expiry and A hiding, but releases on node removal/source replacement.
Live data, attention and heartbeat effects keep running while motion is paused.
The reticle has no painted Focus caption; hover and accessibility still identify
its action. Names appear beside projected glyphs where they fit independently of
hierarchy-panel space, avoiding all current HUD/observation/activity overlays.
The focused-node panel uses free space above/beside the HUD or at the other edge and lists
the full node/session/workspace/agent hierarchy with scrolling, including sampled
geometry. Interactive focus controls are omitted in passive/screensaver mode.

Multiple cards stack in available edge columns. Overflow is pageable; interactive
views have page controls, while passive views advance every twelve seconds.
New transient events return to the first page, with stop notices ahead of blocked
attention, then working and completed cards.
Persistent cards do not expire while off-page. Timed notices still expire after
ten seconds, so an exceptionally crowded screen is not a lossless event log.
Other transient events keep their latest-three bound; activity history is bounded
separately and persistent attention/work/completion take priority over expired/old history.
Activity boxes fit their actual heading, names and compact reticle control, with modest padding. Only
unusually long content scrolls within a capped card; selection details scroll
within their bounded observation card.

Use `--tree` to open the independent existing tree renderer, including Fleet pulse
cards, selection, expand/collapse, scrolling, 400 ms layout easing and conditional
observation pane. It does not initialize the Orb GPU pipelines. Orb supports
clicking a projected mark to inspect observations in a floating card and leader
that stay until dismissed. Click the same mark again, blank space, or Close to
clear selection. Cards do not resize or move the Orb viewport. Project summaries
remain available through Projects in the key panel, grouping exact reported IDs
without inferring Git equivalence or changing project ownership.

The Orb counts panel counts all observed member nodes, sessions, reported workspaces
and agents, plus working/blocked/done agents. Idle/unknown agents remain in the
total, and unresolved workspace placeholders do not inflate workspace counts.
Session counts include the existing default-context branch when its inventory
is unknown; they are not a deduplicated count of native session processes.
These are scoped observation totals, including stale inventory, with an explicit
LAST KNOWN label on disconnect. The separate Fleet pulse cards in `--tree` retain
their dashboard freshness rules. Fixed GPU budgets sample exceptionally large
inventories, keeping ancestor paths and member hubs; an omission notice appears
and totals remain complete. `--tree` exposes the full inventory. Placement uses
scoped identities and reusable slots, not label order.

The viewer opens even when the daemon is offline, reconnects automatically, and retains
the last good observations while disconnected. Receipt timestamps become stale
after 30 seconds; a quiet, healthy stream remains connected.

The logical coordinator root shows the verified upstream instance ID/version,
separately from the local serving daemon. Its execution role remains an ordinary
node, counted once. Older observers show an explicit unknown coordinator root.
Coordinator data is an optional property of the existing observation response;
the service, RPC methods and API version remain unchanged. Rebuild/restart the
Go daemon as well as the viewer to obtain verified coordinator metadata.

Newly observed advancing node last-seen timestamps trigger a 3 second eased
purple heartbeat pulse from the child junction, through the node, up to the
coordinator. Pulses follow moving connectors, use a compact root/legend cue when
paths are hidden, and clear on disconnect. Initial snapshots, reconnects,
duplicates and invalid/future timestamps do not replay activity. The stream
coalesces snapshots, so this is observed heartbeat receipt rather than a
lossless animation of every heartbeat. Node rows/details show last-seen age.

Fleet pulse cards above the tree show connected nodes, fresh Herdr nodes,
reported workspaces, total agents across workspaces (including idle/unknown and
unresolved placement), and agents working/blocked/done. Fresh inventory follows
the dashboard's session/readiness/30-second rules; the control root and orphan
workspace placeholders do not inflate counts. Default/named contexts remain
scoped observations. Unavailable live counts show em dashes with explicitly
last-known notes; a valid empty live fleet shows zero. Cards wrap and animate
their layout over 400 ms. Summary/overview scrolling stays separate from tree
scrolling; the overview and narrow observation pane are bounded on short windows.

The native winit/wgpu/egui-wgpu shell is adapted from
[clarkezone/wgputests](https://github.com/clarkezone/wgputests/tree/56551e298420764e43d6851efc9a2702273af0e1)
commit `56551e298420764e43d6851efc9a2702273af0e1`. The Orb geometry/key/renderer and shaders are adapted from wgputests main
commit `3c0eb41fdc86bc19d7469bb4c2617aa01f5194ae`. The Orbital Sphere source
is adapted from ThreeUI under its MIT license (see [notices](THIRD_PARTY_NOTICES.md)).
Linux keeps Wayland/X11/links support and disables the optional clipboard worker
that crashed on teardown during the prototype; numeric controls and legacy
viewer behavior remain available, with no lab simulation or other gallery scenes. Window
and GPU work remain on the main thread; RPC and projection run on a worker.

## Build and run

```sh
go build -o herdr-mesh ./src/cmd/herdr-mesh
cargo build --manifest-path visualizer/Cargo.toml --locked --release
./herdr-mesh start
./visualizer/target/release/herdr-mesh-visualizer
# Existing tree presentation:
./visualizer/target/release/herdr-mesh-visualizer --tree
```

Use `.exe` filenames on Windows. Existing managed installations need a daemon
restart with the updated Go binary to expose the new observer service. Retain
the original enrolled state directory when replacing or moving a daemon binary.
The Rust executable can be copied independently; it never enrolls a Tailscale
identity, embeds Go, or starts/stops the mesh.

The default endpoint is `127.0.0.1:8790`. For an occupied port or another managed
installation on the same computer:

```sh
herdr-mesh shutdown
herdr-mesh start --visualizer-port 8791
herdr-mesh-visualizer --port 8791
```

`init` and `join` also accept `--visualizer-port`. This override applies to one
launch and is never saved in enrolled configuration. Repeat it after a restart.
An explicit override while already running is rejected. Failure to bind the
optional observer port leaves the mesh running and writes a warning to
`daemon.log`. Port 8790 serves only `LocalObserver.GetInfo` and `WatchNodes`;
the private Fleet control gateway stays separate. There is intentionally no
local authentication or TLS: any local process can read these observations.
The listener cannot bind a wildcard or non-loopback interface.

Each workspace displays its reported project affiliation, or an explicit
unassigned/unresolved label. A separate project summary groups exact reported
IDs across nodes and shows scoped workspace-observation and distinct-node counts.
Counts include retained observations and count default/named session contexts
separately; the current snapshot cannot prove those contexts alias one physical
workspace. These IDs are operator declarations, not verified Git equivalence.
Independent project configuration/readiness and projects with no observed
workspace are not in this stream. Missing references stay unresolved. Agent project overrides appear in
details without relocating an observed workspace. Names are display labels;
identities remain scoped by node, native session/incarnation, workspace and tab.

## Windows screensaver

The Windows screensaver uses the same shell, renderer and observer worker.
Normal `.exe` behavior and Linux/macOS builds remain unchanged. Build it with:

```powershell
cargo build --manifest-path visualizer\Cargo.toml --locked --release --features screensaver --bin herdr-mesh-screensaver
Copy-Item visualizer\target\release\herdr-mesh-screensaver.exe visualizer\target\release\herdr-mesh-visualizer.scr
Start-Process -FilePath .\visualizer\target\release\herdr-mesh-visualizer.scr -ArgumentList /s -NoNewWindow
```

`/s` covers every monitor, hides the cursor and exits on keyboard, mouse button,
wheel/touch, or mouse movement of 8 physical pixels after a one-second startup
grace period. Focus loss outside its own windows, suspend, session lock and
display-topology changes also close it.
With `--tree`, the live tree automatically scrolls. If no live stream is available, it shows
only **Daemon not available**, never retained fleet data. Reconnection is automatic.

`/p HWND` (or `/p:HWND`) embeds a non-activating preview in a Windows-provided
parent window and follows its size/lifetime. `/c [HWND]` (also `/c:HWND`) shows
configuration information; no settings are persisted in this first cut. A `.scr`
without arguments shows configuration; a normal `.exe` without arguments opens
the interactive viewer. Switches are case-insensitive; manual launches can
append `--port N`. Windows-configured launches use default port 8790.
For manual `.scr` switch tests, use direct process execution as above: a
ShellExecute launch can invoke the registered file association with `/S`
instead of the requested arguments. Windows passive modes use opaque
Direct3D 12 HWND surfaces; the regular viewer keeps its existing backend selection.

For a separate screensaver archive/checksum:

```powershell
python scripts\package-visualizer.py --binary visualizer\target\release\herdr-mesh-screensaver.exe --version 0.1.0 --screensaver
```

Extract the single `.scr` to a stable location. Right-click **Install** to open
Windows Screen Saver Settings, then explicitly select the saver, wait time and
sign-in-on-resume policy. System-wide deployment into `%WINDIR%\System32` requires
administrator approval; builds do not install or activate anything. Windows,
not this app, handles secure resume. The saver never starts/enrolls the daemon.
Live fleet names and statuses are visible while running: consider this before
enabling it on a publicly visible display.

See [the screensaver specification](../docs/visualizer-screensaver.md) for scope,
error behavior, compatibility and acceptance requirements.

## Verification

```sh
cargo fmt --manifest-path visualizer/Cargo.toml -- --check
cargo clippy --manifest-path visualizer/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path visualizer/Cargo.toml --locked
HERDR_VISUALIZER_TEST_BINARY="$PWD/visualizer/target/release/herdr-mesh-visualizer" \
  go test ./src/internal/meshlocal -run TestRustObserverWireContract -count=1
```

`herdr-mesh-visualizer --check [--port 8790]` performs the same handshake and
snapshot decoding without requiring a GPU/window. CI builds/tests on Windows,
macOS and Linux and runs the Go-to-Rust wire check, including a 5 MiB response.
This does not substitute for graphical acceptance on the established wgputests
machines. Test window launch/close, resize/DPI changes, scrolling/selection,
daemon loss/restart, named sessions, empty fleet and stale snapshots there.

Orb markers, halos and branch widths use logical display points, matching the
HUD, selection targets and callouts. The active window's pixels-per-point value
is applied each frame, including fractional scaling, egui zoom and monitor
changes. Resize still fits the world-space constellation to the available area.

An opt-in native GPU regression reads rendered pixels at 100%, 125%, 150%, 200%,
300% and 400% scaling to check marker area and branch width:

```sh
cargo test --manifest-path visualizer/Cargo.toml --locked --bin herdr-mesh-visualizer gpu_marker_area_and_branch_width_follow_display_scale -- --ignored
```

The regular tests cover logical marker size limits, projected positions and
clipped fractional-DPI viewports without needing a GPU. Actual monitor changes
and Windows preview/fullscreen presentation remain manual acceptance checks.

Package the host release binary with
`python scripts/package-visualizer.py --binary <path> --version <version>`.
Viewer archives and checksums go into `dist/visualizer/`, separately from the
existing one-binary Go CLI archives. No Rust or Go toolchain is required on a
target computer running the built executable; normal platform graphics/window
libraries and a compatible GPU driver remain required.

[Phase 2 plan, adversarial review and validation](../docs/orb-visualizer-phase2.md).

Viewer and screensaver archives include the Orb third-party notices alongside
the single executable. The notice file is not a runtime dependency.

[Floating Orb panels and persistent activity: requirements, review and validation](../docs/orb-floating-panels.md).

[Persistent Block specs, review and validation](../docs/orb-persistent-block.md).

[Persistent Completed follow-up specs, review and validation](../docs/orb-persistent-completed.md).

[Focus visibility, completion dismissal and icon-only controls](../docs/orb-focus-and-dismissal.md).
