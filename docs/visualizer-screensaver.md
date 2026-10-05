# Windows visualizer screensaver specification

## Scope and compatibility

The screensaver is a Windows presentation mode of the existing Rust viewer,
not a daemon, service, new mesh connection, or replacement UI implementation.
The normal executable keeps its existing window, input, CLI, retained
observations, selection, scrolling and reconnection behavior. Linux/macOS
builds retain that behavior and import no Win32 APIs.

Multi-monitor windows share one wgpu instance, adapter, device and queue, with
separate surfaces and egui renderers. Windows passive modes use Direct3D 12
with opaque HWND swapchains and monitor-sized borderless windows, not a native
fullscreen transition. Native testing exposed Vulkan driver access violations
and Direct3D swapchain creation failures with the latter transition. Reapply
the physical monitor size after window creation to avoid startup DPI rescaling.
Validate presentation capabilities for each monitor. Normal viewer backend
selection and Linux/macOS rendering remain unchanged.

The console-free Windows screensaver executable shares the viewer source and
renderer and is distributed as `herdr-mesh-visualizer.scr`, separately from
the existing viewer archive. Installation and activation are explicit operator
actions; builds must not change Windows screensaver settings or daemon state.

## Launch contract

- `/s` (case-insensitive): one borderless, always-on-top window per monitor,
  hidden cursor, shared observer worker, independent renderers.
- `/p HWND` or `/p:HWND`: embed one non-activating child window in a valid
  preview parent's client area. Track parent resizing and exit when it closes.
  Invalid/zero handles fail explicitly; never fall back to a full-screen window.
- `/c`, `/c HWND` or `/c:HWND`: show an optionally parented configuration
  information dialog. This first cut has no persisted settings. The Windows
  configured screensaver uses the default localhost port 8790; manual launches
  may append `--port N`. Do not claim that manually selected ports are persisted.
- With no arguments, a `.scr` shows configuration; the ordinary `.exe` still
  opens the interactive viewer.
- Existing `--port`, `--check`, `--help` and error handling remain available.
  Reject conflicting screensaver modes and `--check` with a screensaver mode.
  Reject Windows switches on other platforms.

## Presentation and lifecycle

Reuse the live mesh tree and Fleet cards. Screensaver/preview input is not
forwarded to egui, preventing selection, collapse and clipboard actions.
Automatically scroll an overflowing tree slowly back and forth, with pauses
at its ends. Normal viewer scrolling remains directly controlled by input.
When there is no live observation stream, show exactly `Daemon not available`
instead of retained nodes, cards or diagnostic details. The existing bounded
worker continues retrying; live rendering resumes when observations return.
Fit the unavailable message within small preview viewports without clipping.

Full-screen mode exits on keyboard press, mouse button, wheel/touch input,
meaningful mouse movement, close request, focus loss outside its own windows after startup, suspend,
session lock, or monitor-topology change. Windows session notifications and
power messages supplement winit lifecycle events. Ignore initial/synthetic
pointer movement for one second; subsequent movement of at least 8 physical
pixels dismisses.
Defer startup focus-loss checks until both the one-second grace period and
100 ms activation-settling delay have elapsed. Then check whether any saver
window owns the foreground; a persistent external focus loss must dismiss
without requiring another focus event.
Preview mode never dismisses on pointer input or steals focus.
On exit, stop and join the existing observer worker and release GPU/windows.
Do not enroll, restart, stop or mutate the daemon. Do not implement passwords
or claim the app locks Windows: Windows owns the secure-resume policy.

## Build and acceptance

Keep existing three-platform format/clippy/test/release and wire-contract CI.
Add Windows-only screensaver build/package checks without changing other
platforms' dependencies or artifacts. Package exactly one `.scr` per separate
archive with SHA-256.

Automated coverage: existing viewer behavior, argument/default/conflict/handle
validation, pointer debounce and thresholds, automatic-scroll bounds, offline
presentation, packaging, and Win32 code compilation. Native acceptance:
ordinary viewer selection/resize/scroll, `/s` launch on all monitors and input
dismissal, `/p` real embedding/resize/parent-close, `/c`, unavailable daemon,
reconnection, DPI/display changes, and clean shutdown. Linux/macOS native CI
must stay green; a local Windows build is not evidence of native Linux/macOS
or multi-monitor acceptance.

## Implementation and acceptance evidence

The Windows ARM64 viewer and console-free screensaver release builds passed
format/lint checks and 45 distinct Rust tests (shared binary tests also run
through the screensaver entry point), including invisible native HWND
validation, sent suspend/lock messages and shutdown-hook reference cleanup.
Five packaging tests cover the three existing viewer platforms, separate .scr
packaging, unsupported machines, and Python emulation versus PE architecture.
The original Go-to-Rust observer contract also passed.

Linux x86_64 and macOS ARM64 viewer/all-targets cross-compilation and clippy
checks passed. These are not native graphical execution or remotely run CI.
Native Windows ARM64 graphical acceptance passed using the existing live
daemon, without starting another daemon:

- Two visible, topmost windows exactly cover 3440x1440 at 100% DPI and
  2304x1536 at 125% DPI. Captures were visually inspected on both monitors.
- Live mode shows fleet/tree observations; an intentionally unused observer
  port shows only `Daemon not available` on both monitors.
- Keyboard and meaningful pointer movement dismiss both windows with exit 0.
- Focus transfers between the saver's monitor windows keep it running;
  focusing a controlled external dialog dismisses it cleanly. Check native
  foreground ownership after a 100 ms activation-settling interval, not an
  immediate per-window focus-state snapshot.
- `/p` embeds in a controlled real Win32 parent HWND, remains open after input,
  fits its client area before and after resizing, and exits cleanly when the
  parent closes. Foreground captures confirm the actual embedded rendering.
  A settings-sized 152x112 parent also shows the complete unavailable message.
- `/c` shows the default-port/no-persistence/Windows-owned-policy information
  dialog and exits cleanly after OK.
- The ordinary viewer still selects entities, opens observations, collapses
  and expands branches, scrolls and retains selection across resizing.
  The existing daemon snapshot check succeeds afterward.

Native sent-message tests cover session-lock/suspend handling and hook cleanup;
physical lock, suspend and monitor hot-plug were not performed on the shared
host. Initial acceptance did not change Windows Settings activation; the
controlled preview host exercised the real embedding contract.

Subsequent operator-authorized per-user installation also verified the actual
Windows Screen Saver Settings application: the installed binary matches the
release hash, Windows selects it, and the live child preview renders inside
the settings monitor. The existing wait time and sign-in-on-resume policy were
preserved. Installation does not replace, restart or enroll the daemon.

For manual switch tests, execute the `.scr` directly (for example PowerShell
`Start-Process -NoNewWindow -ArgumentList ...`), not through ShellExecute:
the Windows `.scr` file association can replace arguments with `/S`.
Hardware-rendered captures should use screen capture of a foreground window;
PrintWindow can omit the GPU content, and an obscured screen capture can show
another application's pixels instead.
