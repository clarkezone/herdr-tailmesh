# Persistent latest agent outcomes

The visualizer retains one latest outcome per scoped agent independently of the
agent's live state. This fixes Completed disappearing on Done → Idle while also
preserving real direct Working → Idle stops. PR #20 remains open for user testing.

## Functional specification

- Working → Done → Idle retains unread AGENT COMPLETED.
- Direct Working → Idle retains WORK STOPPED — IDLE. Done → Idle preserves
  Completed and does not create a second stop. Startup Idle has no invented history.
- Later Working preserves the previous unread outcome beside its live work card.
  The next Done or direct Working → Idle supersedes the previous outcome with a
  fresh instance. One latest outcome per agent, without a historical queue.
- × dismisses only the current instance. A stale click/dismissal cannot hide a
  later outcome. Preserve per-agent independence, Focus, leaders, sci-fi
  reveal/retract, stop-arrival priority, pagination and W for live Working only.
- Journal observed outcomes immediately, dismiss on click, restore both pending
  outcomes and dismissed-instance suppression before publication. Relaunch while
  Idle, Working or missing must not erase an observed outcome or replay a dismissal.
- Scope by verified coordinator and full node/session/incarnation/workspace/tab/
  agent identity. A changed source cannot attach history to another agent.
- Show outcome plus actual current state/no-longer-observed and hierarchy labels.
  Markers/counts stay truthful; daemon, RPC, enrollment, project model and tree
  remain unchanged. Unseen work while closed cannot be inferred.
- Preserve unread history at storage capacity; disclose failures/omitted cards
  and retry transient errors. Unverified history remains session-only.

## Implementation specification

- Receiver-owned tracker compares every accepted scene before UI coalescing;
  publish latest outcomes separately from raw Done/Idle state episode maps.
- A presentation key appends `outcome` to the full real agent key. Live Working
  and its previous outcome can coexist; Focus and anchors resolve the real key.
- Use a versioned local protobuf journal separate from legacy acknowledgements:
  verified source, agent identity, fresh token, kind, four display names,
  observation stamp and dismissed flag. Migrate saved Done acknowledgements.
- Bounded locked read/merge/write plus atomic replacement preserves other scopes.
  Conditional tokens reject stale writers/acknowledgements. A newer proven stop
  observed during an initial read failure can replace older disk history using
  its observation stamp; preserve that condition through a queued click/retry.
- Bound the journal to 8,000 records/16 MiB. Recycle only dismissed records outside
  current Done at capacity. Do not erase unread history to admit another record.
- Reconcile retained cards independent of raw status/removal/GPU sampling, remove
  superseded instances before admission, preserve reveal identity on unchanged
  snapshots, and retract acknowledged cards. Bound presentation to 8,000 and
  disclose omission separately from complete live fleet totals.
- Extend existing diagnostics with outcome availability/supersession, journal
  readiness/error/recovery and per-instance click/save; keep raw state counts and
  retained history distinct. Avoid extra animation-frame records.

## Adversarial review and validation

- Regression coverage uses the exact user sequences, receiver coalescing, stale
  clicks, real-file restart/dismissal, actual UI text/keyboard controls, truthful
  live counts/markers, source/removal and production tonic/protobuf reception.
- Review caught and fixed a failed-read retry hazard: a click could drop the
  pending supersession condition and restore the older completion after unlock.
- Concurrent scope merge, stale writes/dismissals, corrupt/empty/future/oversized
  file preservation and explicit capacity admission are tested.
- Source gaps cannot reconstruct unseen work. Native Windows/macOS graphical and
  user-repro acceptance remain separate from portable tests. A CPU `drawn` log is
  not a GPU/display guarantee.
- Final formatting, strict clippy, Rust tests, launcher builds, native receipts and
  platform CI are recorded in PR #20 and the AI Core adversarial review.
- Leave the PR open for user testing; merge requires an explicit request.
