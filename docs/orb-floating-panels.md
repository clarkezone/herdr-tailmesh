# Orb floating panels and persistent activity

## Requirements

- Merge the reviewed phase 2/DPI/ring improvements first. PR #11 is squash-merged
  as `3a5ed89`, after all six platform CI jobs passed on `50231ea`.
- Remove redundant Orb top cards/title/control area. Center the world viewport
  with symmetric margins and keep its bounds fixed when a panel appears.
- Preserve the independent tree presentation through `--tree` for both launchers;
  keep its header, Fleet pulse, layout, scrolling and observations unchanged.
- Put the visual key and seven totals into one bounded rectangle. Restrict the
  semitransparent background to that rectangle and gently move it vertically.
  Include a coordinator leader, connection/retained label, project summary access
  and any detail-sampling disclosure within the same panel.
- Selection uses a floating observation card and joining line, with no timeout.
  Toggle by clicking the same object, blank space or Close. Preserve the full
  scoped path, observed status, freshness, last-seen and reported details.
- Each working scoped agent has its own persistent callout; simultaneous work
  must not be replaced by a latest-three queue. On an observed stop, show the
  actual reported state using the regular ten-second notice/fade and expire it.
  Never label idle, blocked, disconnected or disappeared agents as completed.
- Initial/reconnect snapshots may display current working state but cannot
  invent historical starts/stops. Stale/disconnected working state is last known.
- Keep panels/cards inside logical-coordinate viewports and prevent overlap or
  click-through. Provide disclosed pagination when the viewport cannot fit all
  activity. Passive mode rotates pages and has no interactive controls; without
  a live stream it still displays only `Daemon not available`.
- No daemon, observer wire, data model, authentication or enrollment changes.

## Implementation

`orb_ui` reserves a full symmetric scene rect instead of placing header and side
panels around it. `orb_panels` owns the bounded HUD, project/selection cards,
leaders, hit masks and activity placement. The key and stats expose translations
so their existing measured layout moves as one panel without a full-width strap.
Motion uses a 48-second sine cycle: up to twelve logical points for the HUD and
eight for cards, reduced to available room. Mark sizing, surface-tangent ring
transforms, shaders, pulse geometry and display-scale handling are unchanged.

The Orb presentation adapter keeps activity by scoped projection key, independently
of the generic transient deque. Existing working cards update names without
resetting entrance/identity; fresh state deltas replace the scoped card with a
timed actual-state notice. Reconnect/source changes establish baselines, and
missing records cannot anchor to reused geometry slots. Activity history is
bounded to 8,000 cards, trimming oldest timed history once per reconciliation;
active work has priority. Existing geometry detail sampling still applies and
complete totals remain independent of rendered detail.

Cards occupy edge-column slots outside HUD/observation bounds. Long content
scrolls; text is laid out only for visible cards. Up to 64 slots are considered,
and pages disclose the remaining activity. New timed notices reset to page one
and take precedence there; passive pages otherwise advance every twelve seconds.
Working state remains retained off-page. Timed notices expire at ten seconds even
when off-page; this presentation does not claim lossless event delivery. Small
previews use abbreviated counts; extremely short/narrow views may have no room
for activity or observation cards.

## Adversarial review and fixes

- Reconnects and stale comparisons could fabricate completions: preserve working
  cards as last known, drop superseded baseline state quietly, and emit truthful
  removal notices only on live deltas.
- Scoped slots can be reused: resolve activity leaders by current scoped key,
  rather than a retired numeric slot. Stop notices retain captured names.
- A later state change could leave contradictory cards: replace the same scoped
  activity rather than adding another timed notice for it.
- Persistent work could hide a completion until expiry: give timed notices
  first-page priority and reset the page when a new notice arrives.
- Render admission could be mistaken for membership: silently drop cards that
  lose sampled detail while still reported, and keep the sampling disclosure.
- All-fleet text measurement and repeated history scans could amplify work:
  lay out only displayed cards and trim bounded history once per snapshot.
- Leaders painted after another card could cross its text: draw all leaders
  first, then cards and the key panel. Include all actual panel bounds in hit masks.
- Card drift could clip/overlap: reserve margins for drift, use bounded slots and
  adapt tiny views; long text scrolls inside each card.
- Observation could shift the sphere or dismiss underneath a panel: keep the
  centered viewport invariant and mask panel clicks. Same-object toggling and
  blank-space dismissal are exercised with actual egui pointer events.
- Legacy presentation could inherit Orb chrome: keep its UI/GPU routing separate.
  No changes to tree UI, Fleet pulse renderer or launch behavior are required.

## Validation

- Formatting and strict all-target clippy with the screensaver feature passed.
- All 68 distinct portable tests passed (118 executions across library, both
  launchers and observer integration). Two launcher copies of the optional GPU
  readback test remain ignored; shader/scaling code is unchanged in this revision.
- New regressions cover concurrent working cards beyond sixty seconds, independent
  stop expiry/status, reconnect/departure honesty, admission sampling, bounded HUD
  motion, slot separation, compact totals, persistent observation, centered bounds
  and same-object/blank-space pointer toggling. Existing identity, GPU budget,
  receipt, tree and screensaver lifecycle tests remain green.
- Native Linux live-stream screenshots show the centered Orb, bounded key/count
  background, readable working callout and connected leader. Real fleet captures
  stay local and are not committed. Native Windows/macOS graphical, mixed-DPI and
  screensaver/preview acceptance remain manual platform checks.
- Optimized Linux builds for both launchers passed. The real local observer
  handshake/snapshot check passed, and a separately launched `--tree` native
  window confirmed the preserved text/tree presentation and Fleet pulse cards.
  Platform PR CI runs separately; graphical Windows/macOS acceptance remains open.
