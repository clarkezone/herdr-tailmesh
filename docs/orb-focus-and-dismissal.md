# Focus visibility and completion acknowledgement

User-reported follow-up to PR #14: Focus names depended on the startup key hiding;
the hierarchy could become inaccessible; Completed callouts needed dismissal;
and the Focus control should show only its reticle/brackets.

## Functional contract

- Project focused-node names independently of the hierarchy panel's size. Names
  must appear during Focus with K/N visible; retain bounded collision avoidance.
- Put the hierarchy in a free region above/beside the HUD or at the other edge.
  Reserve all current HUD, hierarchy, observation and activity geometry before
  projecting labels, so later-painted flyouts cannot cover them. Small views
  retain bounded fallback and Escape exit rather than overlapping panels.
- Completed callouts offer their own icon-only Dismiss: a bracketed × matching
  the Focus reticle's rails, with animated hover/keyboard illumination and an
  accessible button label/hover help. Retract with the existing sci-fi animation,
  then remove their widgets, hit masks and pagination entries. Counts, agent
  state, working/blocked cards, selection and Focus remain independent.
- A dismissal acknowledges the current observed Completed episode, scoped to
  node/session/incarnation/workspace/tab/agent plus an explicit completion ID.
  Track each accepted snapshot before UI coalescing: leaving Done/removal retires
  the ID, and returning to Done allocates another. The UI need not render Working
  to rearm the next completion. Retain it across unchanged snapshots,
  reconnect, renames, freshness aging and geometry/history sampling. Clear it
  on an observed state change, removal or source replacement; a later completion
  appears again. Dismissals are local to the viewer launch, never mesh mutations.
- Show only the checkbox reticle, brackets and checked illumination. Keep the
  Focus hover hint and pointer/Tab/Space/Enter/accessibility semantics.
- Preserve passive screensaver behavior, independent --tree, observation protocol
  and current persistence/admission/count invariants.

## Implementation and adversarial review

- Separate overlay placement, hierarchy rendering and projected-name rendering;
  projected names cannot sit after a compact-panel early return. Choose a free
  vertical segment at candidate edge/HUD-adjacent positions. Include activity
  drift bounds in label reservations before painting.
- Keep scoped dismissal timestamps in Panels, prune against complete observed
  Done membership and matching completion IDs when revision/epoch changes, and
  clear on source reset. Receive-time IDs live beside the accepted scene under
  the client's existing mutex; retain only current Done entries and a serial.
  Share the immutable ID map with frames. Source replacement gets new IDs;
  reconnect/unchanged Done preserves IDs. Activity compares completion IDs so a
  coalesced Working/Done cycle restarts its reveal and page identity. GPU/history
  admission event serials do not define acknowledgement lifetime.
- Dismiss is a local acknowledgement, not setting the mesh agent to Idle/Done or
  removing its observation. Passive launchers offer no interactive dismissal.
- The icon refinement removes stock button chrome and text. Draw a green × and
  cut-corner rails in logical display points, bounding them to narrow footer
  widths. Preserve disabled/reveal clipping, Button metadata, hover help and
  Tab/Space/Enter. Existing real pointer coverage clicks the icon; native
  inspection confirms independent acknowledgement and unchanged counts/Focus.
- Shorten the reticle width and keep its illumination inside its own bounds.
  Test real pointer and keyboard input without depending on a painted Focus label.
- Adding the hierarchy shifted egui auto widget IDs and dropped keyboard focus
  from activity controls. Give floating child UIs explicit stable IDs. An
  integration regression reproduces the failure before the fix and exercises
  Tab/Space to focus, Enter to return, then Tab/Enter to acknowledge completion.
- Regressions must exercise K/N startup and transitions in shorter logical
  windows, independent labels with an unusably small hierarchy, all overlay
  exclusions, real Dismiss clicks/retraction/hit masks, independent concurrent
  cards/counts/Focus, reconnect/sampling retention and later completion reappearance.

## Validation

Formatting, strict screensaver-feature all-target clippy and 105 distinct portable
Rust tests passed (190 executions across viewer/screensaver launchers; optional
GPU readback excluded). Optimized Linux builds passed for both launchers.

Native Linux synthetic-observer inspection at 1100×600 confirmed projected names
and a separate hierarchy while startup K/N remained visible, icon-only reticles,
keyboard activation and completion dismissal. One of four completion cards
retracted and left pagination while the fleet retained four Completed agents and
Focus stayed active. K hiding relocated the hierarchy without losing labels;
Escape returned to the fleet and the viewer closed cleanly. The temporary native
fixture and screenshots are not shipped.

Cross-platform compilation/tests, wire contract and packaging are checked in PR
CI. Native Windows/macOS graphical and mixed-DPI acceptance remain manual.

## Completion-instance follow-up

The agent-key-only acknowledgement previously relied on the UI seeing a non-Done
snapshot. A regression reproduced a dismissed completion remaining suppressed
when Working was processed without drawing panels. The receive-time IDs also
cover Working and Done both arriving before the renderer reads the latest scene.
Tests cover both paths, same agent IDs on separate nodes, repeated Done, reconnect,
source/removal, real dismissal input, independent counts and sampled readmission.
Native Linux synthetic-observer inspection confirmed a dismissed card returning
after back-to-back Working/Done messages, restoring four retained completions
with independent counts/Focus, K relocation, Escape return and clean close.

This is viewer presentation state; it changes neither the observation protocol
nor agent identity. If the stream omits the intermediate state entirely, there is
no task/completion ID in the current observation to distinguish two Done snapshots.
That delivery question is being reviewed separately; the viewer does not invent
new completions from unchanged Done receipts.
