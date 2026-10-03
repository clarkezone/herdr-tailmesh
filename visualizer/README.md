# Herdr mesh visualizer

A standalone Rust viewer for the managed Herdr mesh daemon. The first cut draws
a spatial coordinator → node → session → workspace → agent tree using GPU
text and lines, with selection, expand/collapse and vertical scrolling (wheel or
middle-button drag). Children stack beneath their parents with a rightward
indent that adapts to the available width; long labels wrap into taller rows.
The tree fits horizontally without panning. On narrow windows the collapsible
observation pane moves above the tree; wider windows keep it beside the tree.
All window content has a 16-point outer margin. The observation pane appears
only while an entity is selected. Click empty tree space or the outer margin to
deselect and let the tree reclaim the space; clicks inside observations retain
selection for reading/copying. An entity removed from a snapshot is deselected.
Layout changes use shared quintic ease-in/ease-out over 400 ms: resize, pane opening/closing,
expansion/collapse, and changed snapshots. New rows fade in; removed/collapsed
rows fade out without accepting clicks. Further changes retarget from the
current position. Connectors and hit targets follow animated row positions.
Text wraps for the destination width and is clipped during transitions; glyphs
are not individually morphed. Scrolling follows input directly.
It opens even when the daemon is offline, reconnects automatically, and retains
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
commit `56551e298420764e43d6851efc9a2702273af0e1`. It retains the Linux Wayland
clipboard startup fix and imports no demonstration scenes or artwork. Window
and GPU work remain on the main thread; RPC and projection run on a worker.

## Build and run

```sh
go build -o herdr-mesh ./src/cmd/herdr-mesh
cargo build --manifest-path visualizer/Cargo.toml --locked --release
./herdr-mesh start
./visualizer/target/release/herdr-mesh-visualizer
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
The live tree automatically scrolls. If no live stream is available, it shows
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

Package the host release binary with
`python scripts/package-visualizer.py --binary <path> --version <version>`.
Viewer archives and checksums go into `dist/visualizer/`, separately from the
existing one-binary Go CLI archives. No Rust or Go toolchain is required on a
target computer running the built executable; normal platform graphics/window
libraries and a compatible GPU driver remain required.
