# Orb object-class callouts

The object-class backlog replaces generic Observation with typed coordinator,
node, session, workspace and agent cards. Its functional/implementation specs and
adversarial review are maintained in ai-core under Object Class Callouts.

- **N/S/W** independently toggle node/session/workspace cards, initially off.
  **F** toggles fleet counts, **A** Working-agent cards, **K** the legend.
  Repeated/released/modified keys and passive screensaver input do not toggle.
- Click an object to open its typed card; click again, blank space, or its
  bracketed × to close it. Bulk and clicked cards deduplicate by full projected
  identity. A closed bulk item stays hidden until its class is toggled off/on
  or the item is explicitly selected again.
- Cards show their actual legend glyph, scoped names, current status/freshness,
  details and child counts. They connect to their exact GPU representation.
  Node cards show that node's herdr-mesh version and its session Herdr versions.
  Missing metadata is unknown; a coordinator version is never a member version.
- The shared bracketed reticle focuses a descendant's node rose. Coordinator
  focus brings the logical root forward in a paused fleet view. The coordinator
  is not an execution node and never changes fleet counts.
- Visible entrances are staggered by 200 ms. Holographic retraction remains
  reversible; layout movements retarget from the displayed position over
  400 ms with easing. Exiting pages have no input controls.
- Selected cards precede bulk; new unread outcomes and attention precede bulk.
  Overflow uses the existing pager. Animation records are bounded to 128; at
  extreme churn, new presentations wait for exits with a disclosed queue count. Only admitted object geometry has bulk cards;
  sampling omissions remain visible. The independent legacy tree is unchanged.
- Typed × closes presentation only. Per-instance historical outcome × remains
  the only acknowledgement action; durable outcomes/counts remain independent.
  Superseded outcomes retire before their replacements, including cached exits.
- Identity/source reset clears object presentations, suppression and focus.
  Missing objects cannot reuse another object's geometry slot.

## Version transport

`NodeView.implementation_version` is additive protobuf field 17, copied from the
node's existing authenticated `Hello.implementation_version`. Display metadata
is bounded to 256 bytes and valid printable UTF-8; invalid values are unknown.
The fleet publishes metadata only after durable persistence and checks current
stream ownership. Reconnect clears prior versions, including legacy empty Hello.
The local read-only observer forwards NodeView unchanged. No protocol-range,
authorization, port, registration, command or project-binding change is needed.
Old daemons remain compatible. Rebuilding the coordinator enables node version
metadata; the existing nodes already supply it in Hello.

## Review and verification

Review targets include shortcut collisions, full scoped deduplication and
suppression, actual pointer close/reopen, historical-outcome supersession,
coordinator focus without a fake node, geometry sampling, bounded pagination,
reversible reveal/reflow, native DPI/resize and legacy daemon compatibility.
Portable regression tests cover those contracts alongside existing durable
outcome, keyboard, native launch-mode and projection tests. Go tests cover Hello
publication, persistence-before-visibility, failed writes, stale streams,
reconnect clearing and observer forwarding. Native inspection and platform CI
receipts are recorded in the ai-core review; platform graphical acceptance
remains separate from portable tests.
