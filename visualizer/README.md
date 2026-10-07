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
The separate counts panel starts visible and stays on until **N** toggles it.
When both are visible they stack without overlap. When either is alone it hugs
the bottom-left margin; showing the other smoothly moves it up before the new
panel unfolds beneath it. Held keys do not repeatedly toggle. Reconnecting does
not reset these preferences or replay the startup key.

Panels and working-agent cards share a holographic reveal: a travelling leader
beam, expanding light rail, upward unfolding aperture, luminous scan edge,
circuit ticks and scan lines. The reverse effect hides them. Text and glyphs keep
their logical sizes throughout. Hidden cards/panels have no clickable footprint.
Tiny previews use abbreviated vertical counts and a bounded key hint when the
full visual key cannot fit. Activity pagination remains in the first callout,
independently of whether the key is showing.

Each observed working or blocked agent has an independent persistent callout with
its real node/session/workspace/agent names, including initial/reconnect snapshots.
**W** toggles working callouts only; blocked attention remains visible. State and
counts, selection and timed notices remain independent of W. Working → blocked
replaces the work card with persistent ATTENTION REQUIRED, and blocked → working
replaces attention with work. A fresh observed transition to done, idle or unknown
shows that actual state for ten seconds. Reconnect/stale comparisons never invent
completion; retained cards are marked LAST KNOWN. Live removal has a truthful
no-longer-observed notice. Multiple cards remain pageable without expiring off-page.

Fresh blocked agents cause a gentle amber node beacon and a repeating circuit/ripple
inside their surface cluster. This uses full observations even when geometry is
sampled, ages with inventory freshness, and remains separate from receipt heartbeat
pulses. Sampled agent callouts anchor to their owning node. Callout history is capped
at 8,000, with attention ahead of working cards and old timed history; capacity
omissions are disclosed independently of K/N/W, while totals remain complete.

Every activity callout and selected node/descendant observation offers **Focus**.
Checking it pauses orbital motion and eases that node's surface cluster forward
with zoom over 1.5 seconds. Another node transfers focus; uncheck, **Escape**, or
**Return to fleet** restores the rotating overview without a camera jump. Focus
survives callout expiry and W hiding, but releases on node removal/source replacement.
Live data, attention and heartbeat effects keep running while motion is paused.
Names appear beside projected glyphs where they fit; the focused-node panel lists
the full node/session/workspace/agent hierarchy with scrolling, including sampled
geometry. Interactive focus controls are omitted in passive/screensaver mode.

Multiple cards stack in available edge columns. Overflow is pageable; interactive
views have page controls, while passive views advance every twelve seconds.
New transient events return to the first page, with stop notices ahead of blocked
attention, then working cards.
Persistent cards do not expire while off-page. Timed notices still expire after
ten seconds, so an exceptionally crowded screen is not a lossless event log.
Other transient events keep their latest-three bound; activity history is bounded
separately and persistent attention/work take priority over expired/old history.
Activity boxes fit their actual heading, names and Focus control, with modest padding. Only
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
