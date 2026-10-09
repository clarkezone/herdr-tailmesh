# Persistent Idle work stops

Some work stops arrive as Working → Idle without Done. Orb now retains that
observed stop so it cannot expire before the operator reads it. This changes
the visualizer only: agent status and fleet Completed counts remain truthful.

## Functional specification

- Track each agent by its full node/session/incarnation/workspace/tab/agent key.
  An accepted Working observation followed directly by Idle creates an independent
  **WORK STOPPED — IDLE** callout with the existing hierarchy names and neutral
  Idle color.
- Retain it without a timeout while that agent remains Idle, until explicitly
  dismissed. Each later Working → Idle cycle rearms its own banner. Reject clicks
  referring to retired cycles.
- Initial Idle, Blocked → Idle, Done → Idle and Unknown → Idle do not create this
  event. Agent removal/reappearance and changed coordinator identity cannot
  carry Working provenance into a new agent/source.
- Track accepted observations independently of rendering: coalesced Working/Idle
  updates must still produce the stop, including before the first Orb frame.
- Cards keep sci-fi reveal/retract, ×, Focus, leaders, paging and LAST KNOWN
  treatment. W affects Working only. New Idle stops receive the same first-page
  priority as new Done episodes.
- Same verified coordinator reconnect retains observed history. Unverified
  reconnect starts a new baseline. An observation gap cannot reconstruct work
  that was never received.
- Idle-stop events and acknowledgements live for the viewer process. Relaunching
  into Idle does not replay or infer a prior stop. Existing durable Completed
  acknowledgement behavior and disk format remain unchanged.
- Use the existing 8,000-activity bound. Blocked and Working have admission
  priority over Done/Idle stops; omitted history is disclosed independently of
  complete fleet totals. Sampled agents use their node anchor when possible.
- No Go, protobuf, RPC, project binding, tree UI, enrollment or daemon change.

## Implementation specification

- In the observation receiver mutex, compare previous/current accepted scenes
  by scoped identity and raw projected status. Preserve tokens only for retained
  Idle events; allocate a fresh stop token on Working → Idle. Reconcile on every
  accepted revision before publishing the latest shared View.
- Keep Idle episode/acknowledgement maps separate from Done storage; share the
  token allocator and expose generic stop lookups for presentation. Acknowledge
  Idle locally after revalidating its current token, without invoking disk saves.
- Reconcile persistent activity from the complete snapshot, including sampled
  agents. Classify persistence and admission priority using status plus the
  receiver-issued token. Keep raw Idle state, marker color and counts unchanged.
- Share stop-arrival ordering, dismissal controls/retraction and flyout reasoning
  across Done and Idle. A received Idle stop before the first panel frame is
  already a proven event and receives priority immediately.
- Extend the existing diagnostic files with Idle creation/retirement, exact
  decimal-string token, priority, visibility/exclusion, dismissal input and
  explicit process-only acknowledgement. Avoid extra per-frame records.

## Adversarial review

- Frame-only tracking loses coalesced Working; detection belongs in the receiver.
  Unit and actual tonic/protobuf stream tests cover back-to-back cycles without
  a renderer.
- Promoting all Idle would fabricate startup stops. Only receiver-issued tokens
  make an Idle activity persistent; tests exclude all non-Working predecessors,
  source changes and removal/reappearance.
- Reusing agent-only dismissal suppresses later work. Tokens are revalidated in
  the receiver; tests cover scoped duplicate IDs, independent dismissal, stale
  clicks and coalesced renewal.
- Sharing Done disk records would confuse two kinds of stop. Idle acknowledgement
  takes a separate session-only path; a real-file test proves saved Completed
  bytes and restart restoration remain unchanged.
- Priority must handle the no-first-frame case as well as normal transitions.
  A crowded 1100×600 actual text-render test checks first-page visibility at
  2, 30, 90 and 300 seconds, W independence, keyboard ×, animated acknowledgement
  retraction and a renewed Idle stop without a Working frame.
- Sampling must not erase event history. Dense full-snapshot coverage tests
  sampled node anchors, stable activity identities, bounded omissions and
  admission for new Blocked/Working agents ahead of retained Idle stops.
- Logs distinguish creation from UI exclusion and local acknowledgement from
  disk save. Receiver and rendered-UI tests check exact tokens, visibility,
  retraction, retirement and absence of fabricated completion/save records.
- CPU-drawn text does not establish native display output. Windows/macOS native
  graphics acceptance and the user's earlier dismissal recurrence remain separate
  from this observed Working → Idle feature.

## Validation

Portable Rust tests, strict all-target screensaver lint, optimized launcher
builds and native smoke receipts are recorded in the PR and AI Core review.
Leave the PR open for user testing; merge requires an explicit request.
