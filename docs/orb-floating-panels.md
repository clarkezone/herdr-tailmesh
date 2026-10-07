# Orb floating panels and persistent activity

## Requirements

- Orb remains centered in a symmetric viewport with a blank outer margin.
- Preserve the independent existing text/tree renderer and Fleet pulse through
  `--tree` in both launchers. No observer protocol, daemon or data-model changes.
- Give the key and counts separate bounded panels with backgrounds restricted
  to their rectangles, gentle vertical drift and leaders to the coordinator.
- Show the key once during the first minute after application launch, including
  its entrance and exit. Then keep it hidden indefinitely. K toggles it manually
  and permanently overrides that startup timer. Another K toggles it off.
- Counts start visible indefinitely. N toggles them independently, without timers.
  Display nodes, sessions, workspaces, agents, working, blocked and complete as
  a vertical list. The key is vertical, with indented states linked to Agent.
- Stack the panels without overlap at the bottom left. The single visible panel
  hugs the margin. When another is requested, slide the existing panel up over
  400 ms with easing, then reveal the newcomer below. Retraction finishes before
  the remaining panel slides down. Reversals preserve animation continuity.
- K/N/W ignore held-key repeats, key releases and command-modified shortcuts.
  Reconnects and source changes preserve visibility preferences and startup time.
  Passive/screensaver mode preserves its existing keyboard-dismiss lifecycle.
- Each currently working scoped agent has a persistent, text-sized callout.
  Multiple cards coexist, with paging when necessary. W hides/restores all working
  cards together; new work respects that preference. States, totals, selected
  observations and timed stop notices are independent of W.
- Panels and activity cards use the same sci-fi reveal/retract: travelling beam,
  expanding rail, unfolding aperture, scan edge, circuit ticks and scan lines.
  Keep text at its normal logical size; hidden UI has no widgets or hit mask.
- On an observed stop, replace that agent's card with the actual reported state
  for ten seconds. Do not invent completion from disconnect, disappearance or
  stale data. Initial/reconnect baselines show current work without replaying
  historical events. Retained work is explicitly LAST KNOWN.
- Selection uses a permanent floating observation card/leader, full scoped path
  and existing freshness/details. Same object, blank space or Close dismisses it.
- Keep panels/cards bounded and separated, including resize and small previews.
  Cap unusually long activity text and scroll it; do not allocate fixed large
  boxes for short names. Disclose sampled geometry independently of hidden HUD.

## Implementation

`orb_ui` holds the fixed Orb viewport and consumes K/N/W input before early
returns for an unavailable stream. Source reset clears scoped selection and page
history, while retaining display preferences. `orb_hud::Controls` owns independent
reversible visibility and the single startup timer. Its normalized f64-clock
phase drives `Reveal` for both HUD and activity; paint is clipped rather than
rescaling glyphs or text. The key finishes its first retraction at second 60.
A manual K cancels that timer, including when pressed before the minute elapses.

`orb_panels` docks two narrow vertical panels, with retargetable 400 ms motions
and a common slow drift. Newly requested panels wait for the existing panel to
move before unfolding. A partly visible reversal keeps its ordering. Reservations
include outgoing panels until their animation ends; hidden panels paint nothing
and block no clicks. Bounded previews prefer complete abbreviated counts and a
key hint to overlapping glyphs. The same beam/rail/scan clips working callouts;
W filters only persistent cards after their retraction completes.

Activity is still keyed by the existing scoped projection identity, with current
key lookup for leader anchors. Text measurement determines each visible box's
width and height, within a 320-point width/240-point height cap. Up to 64 candidates
per page are packed into separated edge columns around HUD and observation bounds.
The first card retains a pager while the HUD is hidden. Cursor history provides
previous/next pages without measuring all fleet text. Timed transitions sort ahead
of persistent work; membership changes reset to page one. Passive mode rotates
pages every twelve seconds. Off-page work stays retained; timed notices expire
at ten seconds even when off-page, so this is not a lossless event log.

Complete totals and activity truth remain in the existing model. W does not
change either. GPU geometry, surface-tangent rings, DPI sizing, receipt pulses,
observer data and the separate tree renderer remain unchanged.

## Adversarial review

- A late first snapshot or reconnect must not replay the intro or reset toggles:
  use application clock and retain Controls across source reset.
- Key repeat can flicker or cancel a manual choice: accept only non-repeat key-down
  events; ignore command modifiers and leave passive input dismissal unchanged.
- Rapid reversal can jump visibility: sample the current reveal before retargeting.
  Delay a completely hidden newcomer until docking completes, and do not swap two
  partly visible panels through one another. Resize clips preserve separation.
- An outgoing key must not cover a counts panel sliding down: reserve it until
  fully hidden. Hidden panels/cards must not leave ghost interaction masks.
- W must not hide completions or change totals: apply it solely to persistent
  activity presentation. Retain underlying agent observations and states.
- New work while W is hidden must respect the global setting, and stale work
  remains LAST KNOWN. Never infer a stop from display visibility.
- Long names must not escape their frame or impose fleet-wide layout work:
  measure only the current bounded page, wrap/cap text and scroll exceptional
  content. Short cards shrink to actual text instead of a fixed large slot.
- Clicking a pager must not change the candidate iterator halfway through a frame:
  capture the frame cursor before building widgets; use the new cursor next frame.
- Touching inclusive rectangle edges can stall placement retries: advance beyond
  the reserved clearance edge, with bounded packing and a regression for it.
- A short stack can consume all drift room: reserve a small motion envelope
  before compacting rows, so both panels still move gently.
- Leaders crossing text are distracting: paint all leaders before any frames/UI.
- Counts must remain visible when the key times out, and pagination must stay
  accessible independently. Geometry sampling disclosure remains outside toggles.

## Validation

The preceding Orb/DPI and persistent-activity changes were verified with portable
Rust checks and native Linux captures. This follow-up adds regressions for actual
K/N/W events, repeat/release/passive behavior, once-only timing and manual overrides,
source-reset preferences, rapidly reversing bounded docks, text-sized packing,
W-independent totals/stops, and pointer-driven pagination with the key hidden.
Native Windows/macOS mixed-DPI, graphics and screensaver acceptance remain manual
platform checks; platform compilation and portable checks run in PR CI.

Final follow-up checks passed: 75 distinct portable Rust tests (132 executions
across both launchers), strict screensaver-feature all-target clippy, formatting
and optimized Linux viewer/screensaver builds. Native live-stream inspection
confirmed the adaptive vertical key, separate complete totals and a content-sized
working card; real W key presses showed its scan/retraction, fully hidden state
and restoration while counts stayed unchanged. The capture helper stopped if
focus left its owned window; remaining K/N docking checks use portable egui tests.
Real fleet screenshots remain local and are not committed.


## Pre-merge adversarial review (2026-10-07)

- Fixed fresh removal notices retaining an old working event serial. That gave
  a newly observed removal old priority and could keep it off-page when the
  candidate order stayed unchanged. Allocate a new presentation notice serial
  on a live removal, preserving scoped identity, captured names, truthful
  no-longer-observed status and the ten-second lifetime. Baselines and sampled
  admission still cannot emit departure.
- Fixed inaccessible activity pagination at the minimum supported card width.
  A wrapping caption consumed the footer and left neither navigation arrow
  visible. Place navigation first with compact spacing, shorten or omit the
  inline caption when necessary, and preserve full counts/page details in the
  button tooltip. Pointer regression verifies both arrows inside a forty-point
  footer and actual next/previous navigation.
- Reviewed source resets, reconnect/stale activity, scoped slot reuse, sampling,
  bounded history/text work, W-independent stop notices/counts, panel reversals,
  resize clipping, selected observations and independent tree/saver lifecycle.
  The daemon protocol, data architecture and GPU geometry remain unchanged.

Removal identity and minimum-width navigation regressions reproduced failures
before the fixes. The full screensaver-feature suite now passes 78 distinct
portable tests (138 executions across both launchers), with strict all-target
clippy and formatting checks. The first-page regression additionally covers
removal without a candidate-order change. Optimized builds and final platform
CI are checked before merge. Native Windows/macOS graphics and mixed-DPI/saver
acceptance remain manual checks.
