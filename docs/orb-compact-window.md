# Compact companion window

Interactive Orb starts in the remembered larger window. Press **T** to switch
to a small borderless companion; press T again or click **Restore** to return.
Drag its title rail to position it. **Ctrl + / −** (including keypad keys)
resizes it by 10%, between 360×240 and 960×720 logical pixels, subject to the
current monitor. Normal and compact geometry are saved independently in
`window-preferences.json` beside the visualizer's existing state files. Launch
always uses the normal view.

The compact view presents activity and selected-object cards as short rows on
the right, with the same animated reveal, focus checkbox and per-instance
outcome close control. Hover a row for full context; click its text to select
the object. The bottom pager shows the visible range and total callouts.
Closing, paging, resizing or changing modes never changes observed counts.
Full hierarchy details and the existing key/fleet/project panels are available
in the larger view; their visibility settings survive switching modes.
Orb markers retain their existing logical sizing. `--tree`, screensaver and
preview do not use companion mode.

## Omarchy

Hyprland 0.55+ Lua IPC floats, pins, sizes and positions this process's viewer
window. Compact mode removes the compositor border and raises the window when
another ordinary window is activated, without changing keyboard focus. It is
visible across workspaces. Fullscreen apps, panels, lock screens and security
surfaces retain compositor precedence. Normal mode is floating and unpinned to
honor its saved size. No Hyprland config files or global bindings are changed.
Monitor work areas, scaling, rotation and disconnected-monitor restore are
handled using compositor coordinates. IPC runs on a separate worker with
bounded reads and timeouts; errors are logged. Other Wayland compositors do not
provide this adapter and may ignore native topmost/position requests.

## Windows handoff

Shared compact UI, preference persistence and mode switching compile on all
platforms. The winit path already requests decorations, WindowLevel::AlwaysOnTop,
native dragging and logical size changes. Windows native behavior is **not yet
validated** by the Omarchy delivery. The followup agent should:

- Validate topmost across ordinary app activation without stealing focus,
  and normal-mode restoration. If needed, isolate HWND/SetWindowPos behavior
  behind the existing shell/controller boundary.
- Verify title-rail dragging begins on a real mouse press and restore does
  not start dragging; test keyboard and keypad Ctrl + / −.
- Verify 100%, 125%, 150%, 200% DPI, moving across mixed-DPI monitors, normal
  launch and compact resize limits. Sizes are logical, while native positions
  are physical desktop coordinates. Use Windows work areas (not whole monitor)
  for taskbar-aware bounds; select the saved rectangle's monitor when present.
- Validate separate normal/compact size and compact-position restoration,
  disconnecting monitors, negative desktop origins, maximized/minimized state,
  clean shutdown and startup. Do not save transition sizes into the wrong mode.
- Test simultaneous working/blocked/unread Completed/Idle-stop rows, paging,
  selection/focus and durable per-episode × across restart at the minimum size.
- Preserve passive multi-monitor screensaver, preview child HWND and --tree.

This PR remains open for testing; Windows graphical acceptance is a separate
followup, not inferred from CI compilation.

## Adversarial review and validation

Native testing reproduced and fixed repeated floating dispatch clearing pinning,
old Wayland minimum hints clamping the first compact resize, and a final drag
being lost when toggling during resize settling. Geometry capture now waits for
the requested dimensions, reads the outgoing mode before a switch and captures
again on shutdown. Rotated monitor selection uses the same logical extent as
restore clamping. IPC replies have byte limits and total/per-read deadlines.

Portable checks cover minimum row packing, real pointer dismissal of one exact
episode, independent preference replacement/restart, malformed preferences,
size bounds and scaled/rotated/missing-monitor restore. Existing outcome,
protocol, hierarchy, DPI, legacy tree and passive-mode suites remain enabled.
Native Omarchy inspection verifies pin-preserving Ctrl sizing, separate mode
geometry, immediate move/toggle, clean exit/restart, and settled readable rows
at 396×264. Hardware-pointer title dragging and Windows native acceptance
remain manual checks.
