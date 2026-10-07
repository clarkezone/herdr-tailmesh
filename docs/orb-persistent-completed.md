# Persistent Completed callouts

[Completion acknowledgement](orb-focus-and-dismissal.md) adds local Dismiss:
unacknowledged cards persist; acknowledging a current completion retracts its card
until a state change/removal/source reset. Observations and counts stay unchanged.

This follow-up supersedes the ten-second completion lifetime in the Persistent
Block and floating-panel specifications. It changes the Orb viewer only.

## Functional specification

- Every scoped agent whose observed status is `done` has an AGENT COMPLETED
  callout, including the initial snapshot and reconnect baselines.
- Retain that callout while the agent remains Completed. Refresh names and
  freshness without restarting its entrance animation. Show retained stale or
  disconnected state as LAST KNOWN.
- Working, Blocked and Completed replace each other's callout immediately when
  the observed state changes. A fresh transition to Idle/Unknown gets the existing
  ten-second actual-state notice; baseline comparisons cannot invent events.
- Actual removal clears the persistent completed card and may show the existing
  truthful ten-second no-longer-observed notice. Source replacement clears old
  scoped cards. GPU sampling is not removal.
- W hides Working only. Completed cards support the same Focus control, leaders,
  paging and reveal animation as the other persistent cards, in both launchers.
- Persistence means retained while the state holds, including off-page cards;
  the existing 8,000-card capacity remains explicit. Prioritize Blocked, then
  Working, then Completed over timed history. Counts remain complete.
- No completion heartbeat/lighting, daemon, wire protocol, tree-renderer or
  enrollment change.

## Implementation specification

- Centralize persistent-state classification and admission priority in AgentState;
  apply it to admitted geometry, sampled agents, history eviction and omission
  accounting. Include known Done agents in persistent-card demand.
- Preserve scoped activity identities and shared current-state update behavior.
  Completed baselines are state presentation, not replayed completion events.
- Sort persistent card pages by attention, then working, then completed. Keep
  transient actual-state notices first and W's state-specific filter intact.
- Bound sampled admission using higher-priority pending demand; completed history
  yields to working/blocked without refreshing retained cards or starving attention.
- Verify startup/reconnect, unchanged-state longevity, every state transition,
  stale/offline truthfulness, removal/source identity, W independence, sampled
  anchors, dense capacity and unchanged Focus behavior. Run format, strict clippy,
  both-launcher Rust tests, optimized builds and final-head platform CI before merge.

## Adversarial review

- A change only to Activity's persistent flag would omit baseline/sampled completed
  agents and misreport capacity. All admission and retention paths must agree.
- Generalizing W to persistent cards would hide completed/blocked state; filter
  Working explicitly and test actual rendered completion text after ten seconds.
- Baseline Completed cards must show current state without fabricating a receipt
  heartbeat or generic transition; stale cards require LAST KNOWN.
- Completed cards must not crowd out working or blocked attention at the hard
  history limit. Test mixed-state capacity and stable serials across revisions.
- Dense testing exposed GPU-visible cards being recreated after eviction on every
  snapshot, restarting entrances and displacing retained history. Reconcile all
  existing scoped activities from the complete observation before prioritized
  admission. Update downgraded states before eviction so they immediately release
  capacity for new blocked attention. Regression tests cover both cases.
- Final PR review excludes unanchorable agents from pending admission demand:
  agents on nodes awaiting a free rendered-node slot during churn cannot displace visible cards
  and restart their entrances. Their capacity omission remains explicit.
- Source/removal identity and Focus exit remain independent of card persistence.

## Validation

- Formatting and strict screensaver-feature all-target clippy passed.
- 97 distinct portable Rust tests passed (176 executions across both launchers);
  optional native GPU readback tests were excluded.
- Optimized Linux viewer and screensaver builds passed.
- A temporary synthetic variant of the existing Go observer window fixture supplied
  completed agents at startup. Native Linux captures were inspected at startup and
  after eighteen seconds with W hidden: all four scoped completion cards, real
  hierarchy names, Focus controls and leaders remained visible. The viewer closed
  cleanly and the graphical smoke passed. The temporary fixture was removed.
- Windows/macOS native graphical, mixed-DPI and screensaver acceptance remain
  manual. Final-head platform CI must pass before merge; see the PR for receipts.
