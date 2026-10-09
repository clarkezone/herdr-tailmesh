//! Presentation state derived exclusively from the production observation model.
use herdr_mesh_visualizer::{
    client::View,
    diagnostics::json,
    heartbeat::{Pulses, Stamp},
    projection::{Branch, Freshness, Key, Scene},
    summary::summary,
};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::sync::Arc;

pub const MAX_NODES: usize = 128;
pub const MAX_ENTITIES: usize = 8_000;
const CURRENT_BUDGET: usize = 20_000;
const RETAINED_BUDGET: usize = 40_000;
pub const TRANSITION: f64 = 1.2;
pub const PULSE_DURATION: f32 = 3.0;
pub const CALLOUT_DURATION: f64 = 10.0;
pub const CALLOUT_REVEAL: f32 = 0.65;
pub const CALLOUT_FADE: f32 = 1.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Working,
    Blocked,
    Completed,
    Idle,
    Unknown,
}
impl AgentState {
    fn persistent(self) -> bool {
        matches!(self, Self::Working | Self::Blocked | Self::Completed)
    }
    // Higher values win admission when persistent callouts reach the history limit.
    fn priority(self) -> u8 {
        match self {
            Self::Blocked => 3,
            Self::Working => 2,
            Self::Completed => 1,
            Self::Idle | Self::Unknown => 0,
        }
    }
    pub fn color(self) -> [f32; 3] {
        match self {
            Self::Working => [0.05, 0.8, 1.0],
            Self::Blocked => [1.0, 0.32, 0.05],
            Self::Completed => [0.15, 0.95, 0.42],
            Self::Idle | Self::Unknown => [0.48, 0.54, 0.62],
        }
    }
    fn observed(status: &str) -> Self {
        match status {
            "working" => Self::Working,
            "blocked" => Self::Blocked,
            "done" => Self::Completed,
            "idle" => Self::Idle,
            _ => Self::Unknown,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Id {
    Node(usize),
    Session(usize, usize),
    Workspace(usize, usize, usize),
    Agent(usize, usize, usize, usize),
}
impl Id {
    pub fn indices(self) -> (usize, usize, usize, usize) {
        match self {
            Self::Node(n) => (n, 0, 0, 0),
            Self::Session(n, s) => (n, s, 0, 0),
            Self::Workspace(n, s, w) => (n, s, w, 0),
            Self::Agent(n, s, w, a) => (n, s, w, a),
        }
    }
    pub fn seed(self) -> u32 {
        let (n, s, w, a) = self.indices();
        (n as u32)
            .wrapping_mul(4096)
            .wrapping_add((s as u32).wrapping_mul(512))
            .wrapping_add((w as u32).wrapping_mul(32))
            .wrapping_add(a as u32)
    }
    fn child(parent: Option<Self>, slot: usize) -> Self {
        match parent {
            None => Self::Node(slot),
            Some(Self::Node(n)) => Self::Session(n, slot),
            Some(Self::Session(n, s)) => Self::Workspace(n, s, slot),
            Some(Self::Workspace(n, s, w)) => Self::Agent(n, s, w, slot),
            _ => unreachable!(),
        }
    }
    fn cost(self) -> [usize; 2] {
        match self {
            Self::Node(_) => [2, 48],
            Self::Session(..) => [50, 48],
            Self::Workspace(..) => [2, 10],
            Self::Agent(..) => [2, 2],
        }
    }
}
pub fn noise(seed: u32) -> f32 {
    let mut x = seed.wrapping_add(0x9e3779b9);
    x = (x ^ (x >> 16)).wrapping_mul(0x85ebca6b);
    x = (x ^ (x >> 13)).wrapping_mul(0xc2b2ae35);
    ((x ^ (x >> 16)) & 0xffff) as f32 / 65536.0
}
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    (t * t * t * (t * (t * 6. - 15.) + 10.)).clamp(0., 1.)
}
pub fn callout_opacity(age: f32) -> f32 {
    ease(age / CALLOUT_REVEAL)
        * (1. - ease((age - (CALLOUT_DURATION as f32 - CALLOUT_FADE)) / CALLOUT_FADE))
}
#[derive(Clone, Copy, Debug)]
pub struct Life {
    from: f32,
    target: f32,
    since: f64,
}
impl Life {
    pub fn alpha(self, clock: f64) -> f32 {
        self.from + (self.target - self.from) * ease(((clock - self.since) / TRANSITION) as f32)
    }
    fn retarget(&mut self, target: f32, clock: f64) {
        if self.target != target {
            self.from = self.alpha(clock);
            self.target = target;
            self.since = clock;
        }
    }
    pub fn entering(self) -> bool {
        self.target > 0.
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Summary {
    pub nodes: usize,
    pub sessions: usize,
    pub workspaces: usize,
    pub agents: usize,
    pub states: [usize; 3],
}
struct Record {
    key: Key,
    names: Vec<String>,
    state: AgentState,
    freshness: Freshness,
    offline: bool,
}
pub struct Pulse {
    pub origin: Id,
    pub started: f64,
    pub color: [f32; 3],
}
pub struct Event {
    pub serial: u64,
    pub origin: Id,
    pub started: f64,
    pub title: &'static str,
    pub text: String,
    pub color: [f32; 3],
}
pub struct Activity {
    pub state: AgentState,
    pub stop_episode: Option<u64>,
    pub freshness: Freshness,
    pub offline: bool,
    pub event: Event,
    pub persistent: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct ActivityTrace {
    state: AgentState,
    episode: Option<u64>,
    serial: u64,
    persistent: bool,
}
#[derive(Default)]
pub struct Simulation {
    pub entities: BTreeMap<Id, Life>,
    records: BTreeMap<Id, Record>,
    ids: BTreeMap<Key, Id>,
    pub pulses: Vec<Pulse>,
    pub events: VecDeque<Event>,
    /// Current working/blocked/completed agents and timed notices, scoped by identity.
    pub activities: BTreeMap<Key, Activity>,
    pub camera: Option<crate::orb_focus::Pose>,
    pub time: f32,
    pub clock: f64,
    pub live: bool,
    pub callouts: bool,
    pub omitted: usize,
    pub omitted_activities: usize,
    totals: Summary,
    blocked_receipts: BTreeMap<Key, Vec<Stamp>>,
    pub source_generation: u64,
    source: Option<Key>,
    epoch: Option<u64>,
    revision: Option<u64>,
    receipts: Pulses,
    serial: u64,
    completion_episodes: Arc<BTreeMap<Key, u64>>,
    idle_stop_episodes: Arc<BTreeMap<Key, u64>>,
    outcome_agents: BTreeMap<Key, Key>,
    outcomes_managed: bool,
    outcome_dismissed: BTreeMap<Key, f64>,
    outcome_revision: Option<(u64, u64, u64)>,
    wall: Stamp,
    diagnostic_revision: Option<(u64, u64, u64, u64, usize, usize)>,
    diagnostic_activities: BTreeMap<Key, ActivityTrace>,
    pub diagnostic_renderer: Option<u64>,
}
impl Simulation {
    fn trace_activities(&mut self, view: &View) {
        let Some(log) = &view.diagnostics else {
            return;
        };
        let renderer = *self
            .diagnostic_renderer
            .get_or_insert_with(|| log.new_renderer());
        let revision = (
            self.source_generation,
            view.epoch,
            view.revision,
            self.serial,
            self.activities.len(),
            self.omitted_activities,
        );
        if self.diagnostic_revision == Some(revision) {
            return;
        }
        let source_changed = self
            .diagnostic_revision
            .is_some_and(|r| r.0 != self.source_generation);
        if source_changed {
            view.diagnostic(
                "model_source_reset",
                json!({"renderer":renderer, "source_generation":self.source_generation}),
            );
            self.diagnostic_activities.clear();
        }
        self.diagnostic_revision = Some(revision);
        let current: BTreeMap<_, _> = self
            .activities
            .iter()
            .map(|(key, a)| {
                (
                    key.clone(),
                    ActivityTrace {
                        state: a.state,
                        episode: a.stop_episode,
                        serial: a.event.serial,
                        persistent: a.persistent,
                    },
                )
            })
            .collect();
        for (key, trace) in &current {
            if self.diagnostic_activities.get(key) != Some(trace) {
                view.diagnostic("activity_changed", json!({"renderer":renderer, "key":view.agent_key(key), "card_key":key, "state":format!("{:?}", trace.state), "episode":trace.episode, "event_serial":trace.serial, "persistent":trace.persistent, "previous_episode":self.diagnostic_activities.get(key).and_then(|a| a.episode)}));
            }
        }
        for (key, trace) in self
            .diagnostic_activities
            .iter()
            .filter(|(k, _)| !current.contains_key(*k))
        {
            view.diagnostic("activity_removed", json!({"renderer":renderer, "key":view.agent_key(key), "card_key":key, "episode":trace.episode, "event_serial":trace.serial, "reason":"not_retained", "omitted_activities":self.omitted_activities}));
        }
        self.diagnostic_activities = current;
    }
    pub fn summary(&self) -> Summary {
        self.totals
    }
    pub fn key(&self, id: Id) -> Option<&Key> {
        self.records.get(&id).map(|r| &r.key)
    }
    pub fn id(&self, key: &Key) -> Option<Id> {
        self.ids
            .get(key)
            .copied()
            .filter(|id| self.entities[id].entering())
    }
    pub fn state(&self, id: Id) -> AgentState {
        self.records
            .get(&id)
            .map_or(AgentState::Unknown, |r| r.state)
    }
    pub fn opacity(&self, id: Id) -> f32 {
        self.records.get(&id).map_or(0., |r| {
            if !self.live || r.offline {
                0.25
            } else if !r.freshness.is_live(self.wall) {
                0.4
            } else {
                1.
            }
        })
    }
    pub fn activity_retained(&self, key: &Key) -> bool {
        self.activities
            .get(key)
            .is_some_and(|a| !self.live || a.offline || !a.freshness.is_live(self.wall))
    }
    pub fn anchor(&self, key: &Key) -> Option<Id> {
        let key = self.outcome_agents.get(key).unwrap_or(key);
        self.id(key)
            .or_else(|| key.get(..2).and_then(|node| self.id(&node.to_vec())))
    }
    pub fn name(&self, id: Id) -> &str {
        self.records
            .get(&id)
            .and_then(|r| r.names.last())
            .map_or("", String::as_str)
    }
    pub fn motion_time(&self) -> f32 {
        self.camera.map_or(self.time, |pose| pose.orbit_time)
    }
    pub fn blocked_nodes(&self) -> BTreeMap<usize, f32> {
        // Node attention follows complete observation, even when agent geometry
        // is sampled. Cache unique context receipts, then age them every frame.
        self.entities
            .iter()
            .filter_map(|(&id, life)| {
                if !matches!(id, Id::Node(_)) || !life.entering() || self.opacity(id) < 1. {
                    return None;
                }
                let key = self.key(id)?;
                self.blocked_receipts
                    .get(key)?
                    .iter()
                    .any(|stamp| Freshness::Receipt(*stamp).is_live(self.wall))
                    .then(|| (id.indices().0, life.alpha(self.clock)))
            })
            .collect()
    }
    fn prune(&mut self) -> bool {
        let previous = self.entities.len();
        let previous_activities = self.activities.len();
        self.entities
            .retain(|_, life| life.entering() || self.clock - life.since < TRANSITION);
        if previous != self.entities.len() {
            self.records.retain(|id, _| self.entities.contains_key(id));
            self.ids.retain(|_, id| self.entities.contains_key(id));
        }
        self.events.retain(|e| {
            self.clock - e.started < CALLOUT_DURATION && self.entities.contains_key(&e.origin)
        });
        self.activities.retain(|_, activity| {
            activity.persistent || self.clock - activity.event.started < CALLOUT_DURATION
        });
        previous != self.entities.len() || previous_activities != self.activities.len()
    }

    fn slot(
        &self,
        parent: Option<Id>,
        kind: &str,
        cursors: &mut BTreeMap<Option<Id>, usize>,
    ) -> Option<Id> {
        let limit = if kind == "node" {
            MAX_NODES
        } else {
            MAX_ENTITIES
        };
        let cursor = cursors.entry(parent).or_default();
        let Some((slot, id)) = (*cursor..limit)
            .map(|slot| (slot, Id::child(parent, slot)))
            .find(|(_, id)| !self.entities.contains_key(id))
        else {
            *cursor = limit;
            return None;
        };
        *cursor = slot + 1;
        Some(id)
    }
    fn event(&mut self, id: Id, title: &'static str, text: String, color: [f32; 3]) {
        self.serial = self.serial.wrapping_add(1);
        self.events.push_back(Event {
            serial: self.serial,
            origin: id,
            started: self.clock,
            title,
            text,
            color,
        });
        while self.events.len() > 3 {
            self.events.pop_front();
        }
    }
    fn activity(
        &mut self,
        key: &Key,
        id: Id,
        state: AgentState,
        text: String,
        freshness: Freshness,
        offline: bool,
    ) {
        let persistent = self.persistent_state(key, state);
        let stop_episode = match state {
            AgentState::Completed => self.completion_episodes.get(key).copied(),
            AgentState::Idle => self.idle_stop_episodes.get(key).copied(),
            _ => None,
        };
        if let Some(activity) = self.activities.get_mut(key)
            && activity.persistent
            && activity.state == state
            && activity.stop_episode == stop_episode
            && persistent
        {
            activity.event.text = text;
            activity.freshness = freshness;
            activity.offline = offline;
            return;
        }
        self.serial = self.serial.wrapping_add(1);
        self.activities.insert(
            key.clone(),
            Activity {
                state,
                stop_episode,
                freshness,
                offline,
                persistent,
                event: Event {
                    serial: self.serial,
                    origin: id,
                    started: self.clock,
                    title: match state {
                        AgentState::Working => "AGENT WORKING",
                        AgentState::Completed => "AGENT COMPLETED",
                        AgentState::Blocked => "ATTENTION REQUIRED",
                        AgentState::Idle if stop_episode.is_some() => "WORK STOPPED — IDLE",
                        AgentState::Idle => "AGENT IDLE",
                        AgentState::Unknown => "AGENT STATE UNKNOWN",
                    },
                    text,
                    color: state.color(),
                },
            },
        );
    }
    pub fn update(&mut self, view: &View, wall: Stamp, clock: f64) {
        self.clock = clock;
        self.time = clock as f32;
        self.wall = wall;
        self.completion_episodes = Arc::clone(&view.completion_episodes);
        self.idle_stop_episodes = Arc::clone(&view.idle_stop_episodes);
        self.outcomes_managed = view.outcome_notices.is_some();
        self.outcome_agents = view
            .outcome_notices
            .iter()
            .flat_map(|m| m.iter())
            .map(|(k, n)| (k.clone(), n.agent.clone()))
            .collect();
        // Retire superseded outcome instances before admission; an old card
        // must not occupy the replacement's bounded presentation slot.
        if let Some(notices) = &view.outcome_notices {
            self.activities.retain(|key, a| {
                key.len() != 11
                    || key.last().is_none_or(|s| s != "outcome")
                    || notices
                        .get(key)
                        .is_some_and(|n| a.stop_episode == Some(n.episode))
            });
        }
        self.callouts = true;
        let freed = self.prune();
        let Some(scene) = &view.scene else {
            self.live = false;
            self.pulses.clear();
            self.events.clear();
            self.activities.clear();
            self.blocked_receipts.clear();
            self.receipts = Default::default();
            self.revision = None;
            self.trace_activities(view);
            return;
        };
        let source = scene.coordinator.as_ref().map(|b| b.key.clone());
        let changed_source = source != self.source
            || source
                .as_ref()
                .is_some_and(|k| k.last().is_some_and(|id| id == "unknown"))
                && self.epoch != Some(view.epoch);
        if changed_source {
            self.source_generation = self.source_generation.wrapping_add(1);
            self.entities.clear();
            self.records.clear();
            self.ids.clear();
            self.events.clear();
            self.activities.clear();
            self.pulses.clear();
            self.revision = None;
        }
        let baseline = changed_source || self.epoch != Some(view.epoch) || !self.live || !view.live;
        if baseline {
            self.events.clear();
            self.activities.retain(|_, a| a.persistent);
        }
        self.source = source;
        self.epoch = Some(view.epoch);
        self.live = view.live;
        if self.revision != Some(view.revision)
            || freed && (self.omitted > 0 || self.omitted_activities > 0)
        {
            self.reconcile(scene, !baseline && self.revision != Some(view.revision));
            self.revision = Some(view.revision);
        }
        if !view.live {
            self.events.clear();
        }
        self.receipts
            .update(scene, view.live, view.epoch, view.revision, wall, clock);
        self.pulses = self
            .receipts
            .iter(clock)
            .filter_map(|(key, age)| {
                let node = self.id(key)?;
                let n = node.indices().0;
                let origin = self
                    .entities
                    .iter()
                    .find(|(id, life)| {
                        matches!(id, Id::Agent(..)) && id.indices().0 == n && life.entering()
                    })
                    .or_else(|| {
                        self.entities.iter().find(|(id, life)| {
                            matches!(id, Id::Session(..)) && id.indices().0 == n && life.entering()
                        })
                    })
                    .map_or(node, |(id, _)| *id);
                Some(Pulse {
                    origin,
                    started: clock - f64::from(age * PULSE_DURATION),
                    color: [0.7, 0.32, 1.],
                })
            })
            .collect();
        let outcome_revision = (view.epoch, view.revision, view.acknowledgement_revision);
        let retracting = self.outcome_dismissed.iter().any(|(k, t)| {
            self.clock - *t >= f64::from(CALLOUT_FADE) && self.activities.contains_key(k)
        });
        if self.outcome_revision != Some(outcome_revision) || freed || retracting {
            self.reconcile_outcomes(view, scene);
            self.outcome_revision = Some(outcome_revision);
        }
        self.trace_activities(view);
    }
    fn reconcile(&mut self, scene: &Scene, emit: bool) {
        let known = summary(scene, self.wall).known;
        self.totals = Summary {
            nodes: scene.nodes.len(),
            sessions: scene.nodes.iter().map(|n| n.children.len()).sum(),
            workspaces: known.workspaces,
            agents: known.agents,
            states: [known.working, known.blocked, known.done],
        };
        self.blocked_receipts.clear();
        fn attention_receipts(branch: &Branch, receipts: &mut BTreeSet<Stamp>) {
            if branch.kind == "agent"
                && branch.status == "blocked"
                && let Freshness::Receipt(stamp) = branch.freshness
            {
                receipts.insert(stamp);
            }
            for child in &branch.children {
                attention_receipts(child, receipts);
            }
        }
        for node in &scene.nodes {
            let mut receipts = BTreeSet::new();
            attention_receipts(node, &mut receipts);
            if !receipts.is_empty() {
                self.blocked_receipts
                    .insert(node.key.clone(), receipts.into_iter().collect());
            }
        }
        let mut budget = [0; 2];
        let mut retained = [0; 2];
        for id in self.entities.keys() {
            let cost = id.cost();
            retained[0] += cost[0];
            retained[1] += cost[1];
        }
        let mut selected = HashSet::new();
        let mut cursors = BTreeMap::new();
        // Breadth-first admits all node hubs before spending detail on descendants.
        let mut level: Vec<_> = scene.nodes.iter().map(|n| (n, None)).collect();
        while !level.is_empty() {
            let mut next = Vec::new();
            for (branch, parent) in level {
                if selected.len() >= MAX_ENTITIES / 2 {
                    continue;
                }
                let cost = Id::child(parent, 0).cost();
                if (0..2).any(|i| budget[i] + cost[i] > CURRENT_BUDGET) {
                    continue;
                }
                let existing = self.ids.get(&branch.key).copied();
                let Some(id) = existing.or_else(|| self.slot(parent, branch.kind, &mut cursors))
                else {
                    continue;
                };
                let cost = id.cost();
                if (0..2).any(|i| {
                    budget[i] + cost[i] > CURRENT_BUDGET
                        || retained[i] + if existing.is_some() { 0 } else { cost[i] }
                            > RETAINED_BUDGET
                }) || existing.is_none() && self.entities.len() >= MAX_ENTITIES
                {
                    continue;
                }
                let mut names = parent.map_or_else(Vec::new, |id| self.records[&id].names.clone());
                names.push(branch.label.clone());
                budget[0] += cost[0];
                budget[1] += cost[1];
                selected.insert(id);
                let state = AgentState::observed(&branch.status);
                let old = self.records.get(&id);
                let attention = emit
                    && branch.kind == "agent"
                    && branch.freshness.is_live(self.wall)
                    && old.is_some_and(|r| r.state != state && r.freshness.is_live(self.wall));
                let joined = emit && branch.kind == "node" && existing.is_none();
                // Persistent cards reconcile from the complete observation below,
                // independently of which agents have GPU geometry.
                if attention
                    && !self.persistent_state(&branch.key, state)
                    && !(self.outcomes_managed
                        && matches!(state, AgentState::Completed | AgentState::Idle))
                    && !self.activities.contains_key(&branch.key)
                {
                    let title = match state {
                        AgentState::Blocked => "ATTENTION REQUIRED",
                        AgentState::Working => "AGENT WORKING",
                        AgentState::Completed => "AGENT COMPLETED",
                        _ => "AGENT STATE CHANGED",
                    };
                    let text = format!(
                        "{}\nNode: {}\nSession: {}\nWorkspace: {}\nAgent: {}",
                        branch.status, names[0], names[1], names[2], names[3]
                    );
                    self.event(id, title, text, state.color());
                } else if joined {
                    self.event(
                        id,
                        "NODE JOINED",
                        format!("Node: {}", branch.label),
                        [0.85, 0.92, 1.],
                    );
                }
                if existing.is_none() {
                    retained[0] += cost[0];
                    retained[1] += cost[1];
                }
                self.entities
                    .entry(id)
                    .and_modify(|life| life.retarget(1., self.clock))
                    .or_insert(Life {
                        from: 0.,
                        target: 1.,
                        since: self.clock,
                    });
                self.ids.insert(branch.key.clone(), id);
                self.records.insert(
                    id,
                    Record {
                        key: branch.key.clone(),
                        names: names.clone(),
                        state,
                        freshness: branch.freshness,
                        offline: branch.kind == "node" && branch.status != "connected",
                    },
                );
                for child in &branch.children {
                    next.push((child, Some(id)));
                }
            }
            level = next;
        }
        // Count all omitted descendants, not only branches visited before a budget cut.
        fn total(branches: &[Branch]) -> usize {
            branches.iter().map(|b| 1 + total(&b.children)).sum()
        }
        self.omitted = total(&scene.nodes).saturating_sub(selected.len());
        let nodes: HashSet<_> = scene.nodes.iter().map(|b| &b.key).collect();
        let departed: Vec<_> = self
            .entities
            .iter()
            .filter(|(id, life)| {
                matches!(id, Id::Node(_))
                    && life.entering()
                    && !selected.contains(id)
                    && !nodes.contains(&self.records[id].key)
            })
            .map(|(id, _)| *id)
            .collect();
        if emit {
            for id in departed {
                self.event(
                    id,
                    "NODE DEPARTED",
                    format!("Node: {}", self.records[&id].names[0]),
                    [1., 0.45, 0.15],
                );
            }
        }
        for (id, life) in &mut self.entities {
            if !selected.contains(id) {
                life.retarget(0., self.clock);
            }
        }
        // Rendering admission is not mesh membership. Find still-reported keys only
        // when an active card loses its geometry slot; sampling must not emit departure.
        let missing: HashSet<_> = self
            .activities
            .iter()
            .filter(|(key, a)| {
                a.persistent
                    && !self.outcome_agents.contains_key(*key)
                    && !self.ids.get(*key).is_some_and(|id| selected.contains(id))
            })
            .map(|(key, _)| key.clone())
            .collect();
        fn reported(branches: &[Branch], missing: &HashSet<Key>, present: &mut HashSet<Key>) {
            for branch in branches {
                if missing.contains(&branch.key) {
                    present.insert(branch.key.clone());
                }
                reported(&branch.children, missing, present);
            }
        }
        let mut present = HashSet::new();
        if !missing.is_empty() {
            reported(&scene.nodes, &missing, &mut present);
        }
        self.activities.retain(|key, activity| {
            if self.outcome_agents.contains_key(key) {
                return true;
            }
            if activity.persistent && !self.ids.get(key).is_some_and(|id| selected.contains(id)) {
                if present.contains(key) {
                    return true;
                } // Sampled leaves remain observed.
                if !emit {
                    return false;
                } // Baselines cannot assert departure.
                // This is a new timed notice, not the old working-card entrance.
                self.serial = self.serial.wrapping_add(1);
                activity.event.serial = self.serial;
                activity.persistent = false;
                activity.event.started = self.clock;
                activity.event.title = "AGENT NO LONGER OBSERVED";
                activity.event.text.insert_str(0, "Last observed state: ");
                activity.event.color = AgentState::Unknown.color();
            }
            true
        });
        // Reconcile existing cards before admission, so stale priorities and
        // evicted GPU-visible cards cannot restart on each observation revision.
        let mut agents = Vec::new();
        fn collect_agents<'a>(
            branch: &'a Branch,
            names: &mut Vec<&'a str>,
            node: &'a Branch,
            output: &mut Vec<(&'a Branch, Vec<&'a str>, &'a Branch)>,
        ) {
            names.push(&branch.label);
            if branch.kind == "agent" {
                output.push((branch, names.clone(), node));
            }
            for child in &branch.children {
                collect_agents(child, names, node, output);
            }
            names.pop();
        }
        for node in &scene.nodes {
            collect_agents(node, &mut Vec::new(), node, &mut agents);
        }
        agents.sort_by_key(|(b, _, _)| {
            std::cmp::Reverse(self.activity_priority(&b.key, AgentState::observed(&b.status)))
        });
        for (branch, names, node) in &agents {
            let Some(previous) = self.activities.get(&branch.key) else {
                continue;
            };
            let state = AgentState::observed(&branch.status);
            let transition = emit
                && branch.freshness.is_live(self.wall)
                && previous.persistent
                && previous.state != state
                && !(self.outcomes_managed
                    && matches!(state, AgentState::Completed | AgentState::Idle))
                && previous.freshness.is_live(self.wall);
            if self.persistent_state(&branch.key, state) || transition {
                let Some(id) = self.id(&branch.key).or_else(|| self.id(&node.key)) else {
                    continue;
                };
                let text = format!(
                    "{}\nNode: {}\nSession: {}\nWorkspace: {}\nAgent: {}",
                    branch.status, names[0], names[1], names[2], names[3]
                );
                self.activity(
                    &branch.key,
                    id,
                    state,
                    text,
                    branch.freshness,
                    node.status != "connected",
                );
            } else if previous.persistent {
                self.activities.remove(&branch.key); // Baselines cannot invent a resolution.
            }
        }
        self.activities.retain(|key, a| {
            key.len() != 11
                || key.last().is_none_or(|k| k != "outcome")
                || self.outcome_agents.contains_key(key) && a.persistent
        });
        let mut incoming = [0usize; 4];
        for (branch, _, node) in &agents {
            let state = AgentState::observed(&branch.status);
            if self.persistent_state(&branch.key, state)
                && !self.activities.contains_key(&branch.key)
                && self
                    .id(&branch.key)
                    .or_else(|| self.id(&node.key))
                    .is_some()
            {
                incoming[self.activity_priority(&branch.key, state) as usize] += 1;
            }
        }
        let mut evict: Vec<_> = self
            .activities
            .iter()
            .map(|(key, a)| {
                (
                    if a.persistent {
                        self.activity_priority(key, a.state)
                    } else {
                        0
                    },
                    a.event.serial,
                    key.clone(),
                )
            })
            .collect();
        evict.sort_unstable();
        for (priority, _, key) in evict {
            let need: usize = incoming[usize::from(priority) + 1..].iter().sum();
            if self.activities.len() + need > MAX_ENTITIES {
                self.activities.remove(&key);
            }
        }
        for (branch, names, node) in agents {
            let state = AgentState::observed(&branch.status);
            if self.persistent_state(&branch.key, state)
                && !self.activities.contains_key(&branch.key)
            {
                if self.activities.len() >= MAX_ENTITIES {
                    continue;
                }
                let Some(id) = self.id(&branch.key).or_else(|| self.id(&node.key)) else {
                    continue;
                };
                let text = format!(
                    "{}\nNode: {}\nSession: {}\nWorkspace: {}\nAgent: {}",
                    branch.status, names[0], names[1], names[2], names[3]
                );
                self.activity(
                    &branch.key,
                    id,
                    state,
                    text,
                    branch.freshness,
                    node.status != "connected",
                );
            }
        }
        // Disclose the fixed presentation/history bound independently of GPU sampling.
        self.omitted_activities =
            (known.working + known.blocked + known.done + self.idle_stop_episodes.len())
                .saturating_sub(self.activities.values().filter(|a| a.persistent).count());
    }
    fn reconcile_outcomes(&mut self, view: &View, scene: &Scene) {
        let Some(notices) = &view.outcome_notices else {
            return;
        };
        self.activities.retain(|key, a| {
            key.len() != 11
                || key.last().is_none_or(|s| s != "outcome")
                || notices
                    .get(key)
                    .is_some_and(|n| a.stop_episode == Some(n.episode))
        });
        self.outcome_dismissed
            .retain(|key, _| notices.get(key).is_some_and(|n| n.dismissed));
        let mut agents = BTreeMap::new();
        fn scan<'a>(
            b: &'a Branch,
            node: &'a Branch,
            out: &mut BTreeMap<&'a Key, (&'a Branch, &'a Branch)>,
        ) {
            if b.kind == "agent" {
                out.insert(&b.key, (b, node));
            }
            for c in &b.children {
                scan(c, node, out);
            }
        }
        for n in &scene.nodes {
            scan(n, n, &mut agents);
        }
        for (key, n) in notices.iter() {
            if n.dismissed {
                let since = *self
                    .outcome_dismissed
                    .entry(key.clone())
                    .or_insert(self.clock);
                if self.clock - since >= f64::from(CALLOUT_FADE) {
                    self.activities.remove(key);
                }
                continue;
            }
            let state = if n.kind == "done" {
                AgentState::Completed
            } else {
                AgentState::Idle
            };
            let current = agents.get(&n.agent);
            let current_state = current.map_or("no longer observed", |(b, _)| b.status.as_str());
            let text = format!(
                "Current state: {}\nNode: {}\nSession: {}\nWorkspace: {}\nAgent: {}",
                current_state, n.names[0], n.names[1], n.names[2], n.names[3]
            );
            let freshness = current.map_or(Freshness::Stale, |(b, _)| b.freshness);
            let offline = current.is_none_or(|(_, node)| node.status != "connected");
            if let Some(a) = self.activities.get_mut(key)
                && a.stop_episode == Some(n.episode)
            {
                a.event.text = text;
                a.freshness = freshness;
                a.offline = offline;
                continue;
            }
            if self.activities.len() >= MAX_ENTITIES {
                continue;
            }
            self.serial = self.serial.wrapping_add(1);
            let origin = self.anchor(key).unwrap_or(Id::Node(MAX_NODES));
            self.activities.insert(
                key.clone(),
                Activity {
                    state,
                    stop_episode: Some(n.episode),
                    freshness,
                    offline,
                    persistent: true,
                    event: Event {
                        serial: self.serial,
                        origin,
                        started: self.clock,
                        title: if n.kind == "done" {
                            "AGENT COMPLETED"
                        } else {
                            "WORK STOPPED — IDLE"
                        },
                        text,
                        color: state.color(),
                    },
                },
            );
        }
        let demand = self.totals.states[0]
            + self.totals.states[1]
            + notices.values().filter(|n| !n.dismissed).count();
        self.omitted_activities = demand.saturating_sub(
            self.activities
                .iter()
                .filter(|(key, a)| a.persistent && !notices.get(*key).is_some_and(|n| n.dismissed))
                .count(),
        );
    }
    fn persistent_state(&self, key: &Key, state: AgentState) -> bool {
        if self.outcomes_managed {
            return matches!(state, AgentState::Working | AgentState::Blocked);
        }
        state.persistent() || state == AgentState::Idle && self.idle_stop_episodes.contains_key(key)
    }
    fn activity_priority(&self, key: &Key, state: AgentState) -> u8 {
        if self.outcome_agents.contains_key(key)
            || state == AgentState::Idle && self.idle_stop_episodes.contains_key(key)
        {
            AgentState::Completed.priority()
        } else {
            state.priority()
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use herdr_mesh_visualizer::{
        pb::{HerdrEntity, HerdrState, NodeList, NodeView, ServerInfo, SessionView},
        projection::{coordinator, project},
    };
    use prost_types::Timestamp;
    use std::sync::Arc;
    pub fn node(id: &str, status: &str, agents: usize) -> NodeView {
        let state = HerdrState {
            status: "ready".into(),
            workspaces: vec![HerdrEntity {
                id: "w".into(),
                display_name: "Actual workspace".into(),
                project_id: "project".into(),
                ..Default::default()
            }],
            agents: (0..agents)
                .map(|a| HerdrEntity {
                    id: format!("a{a}"),
                    display_name: format!("Actual agent {a}"),
                    workspace_id: "w".into(),
                    tab_id: "tab".into(),
                    agent_status: status.into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let ts = Some(Timestamp {
            seconds: 200,
            nanos: 0,
        });
        NodeView {
            instance_id: id.into(),
            hostname: format!("Actual node {id}"),
            connected: true,
            last_seen: ts,
            herdr: Some(state.clone()),
            herdr_received_at: ts,
            sessions_ready: true,
            sessions: vec![SessionView {
                name: "Actual session".into(),
                incarnation: "one".into(),
                status: "ready".into(),
                herdr: Some(state),
                herdr_received_at: ts,
                ..Default::default()
            }],
            ..Default::default()
        }
    }
    pub fn view(nodes: Vec<NodeView>, revision: u64) -> View {
        let mut scene = project(NodeList { nodes }).unwrap();
        scene.coordinator = Some(coordinator(Some(&ServerInfo {
            instance_id: "coordinator".into(),
            ..Default::default()
        })));
        View {
            live: true,
            scene: Some(Arc::new(scene)),
            epoch: 1,
            revision,
            ..Default::default()
        }
    }
    #[test]
    fn a_real_agent_id_named_outcome_is_not_treated_as_a_history_card() {
        let mut n = node("one", "working", 1);
        n.herdr.as_mut().unwrap().agents[0].id = "outcome".into();
        let mut v = view(vec![n], 1);
        v.outcome_notices = Some(Arc::new(BTreeMap::new()));
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 0.);
        let key = sim
            .activities
            .keys()
            .find(|k| k.last().unwrap() == "outcome")
            .unwrap()
            .clone();
        assert_eq!(key.len(), 10);
        let serial = sim.activities[&key].event.serial;
        v.revision += 1;
        sim.update(&v, Stamp::seconds(201), 2.);
        assert!(sim.activities[&key].persistent);
        assert_eq!(sim.activities[&key].event.serial, serial);
        assert_eq!(sim.activities[&key].state, AgentState::Working);
    }
    #[test]
    fn scoped_identity_rename_order_and_incarnation_are_preserved() {
        let mut sim = Simulation::default();
        let first = view(vec![node("one", "working", 1), node("two", "idle", 1)], 1);
        sim.update(&first, Stamp::seconds(201), 0.);
        let ids = sim.ids.clone();
        assert_eq!(
            sim.entities
                .keys()
                .filter(|id| matches!(id, Id::Agent(..)))
                .count(),
            4
        );
        let mut renamed = node("one", "working", 1);
        renamed.hostname = "Renamed node".into();
        sim.update(
            &view(vec![node("two", "idle", 1), renamed.clone()], 2),
            Stamp::seconds(202),
            2.,
        );
        assert_eq!(sim.ids, ids);
        renamed.sessions[0].incarnation = "two".into();
        sim.update(
            &view(vec![renamed, node("two", "idle", 1)], 3),
            Stamp::seconds(202),
            2.1,
        );
        assert_eq!(sim.totals.agents, 4);
        assert!(sim.entities.values().any(|life| !life.entering()));
        assert!(sim.ids.keys().any(|key| key.iter().any(|s| s == "two")));
    }
    #[test]
    fn real_state_names_receipts_and_reconnect_baselines() {
        let mut sim = Simulation::default();
        sim.update(
            &view(vec![node("one", "working", 1)], 1),
            Stamp::seconds(201),
            0.,
        );
        assert!(sim.events.is_empty());
        assert!(sim.pulses.is_empty());
        let mut changed = node("one", "blocked", 1);
        changed.last_seen = Some(Timestamp {
            seconds: 201,
            nanos: 1,
        });
        let mut v = view(vec![changed], 2);
        sim.update(&v, Stamp::seconds(202), 1.);
        assert_eq!(sim.pulses.len(), 1);
        assert_eq!(sim.activities.len(), 2);
        let text = &sim
            .activities
            .values()
            .find(|a| a.event.text.contains("Actual session"))
            .unwrap()
            .event
            .text;
        for name in [
            "Actual node one",
            "Actual session",
            "Actual workspace",
            "Actual agent 0",
        ] {
            assert!(text.contains(name));
        }
        sim.update(&v, Stamp::seconds(202), 7.);
        assert!(sim.pulses.is_empty());
        assert_eq!(sim.activities.len(), 2);
        sim.update(&v, Stamp::seconds(202), 11.01);
        assert_eq!(sim.activities.len(), 2);
        assert!(
            sim.activities
                .values()
                .all(|a| a.persistent && a.state == AgentState::Blocked)
        );
        assert!(sim.events.is_empty());
        v.epoch = 2;
        v.revision = 3;
        sim.update(&v, Stamp::seconds(202), 12.);
        assert!(sim.events.is_empty());
        assert!(sim.pulses.is_empty());
        v.live = false;
        sim.update(&v, Stamp::seconds(202), 12.1);
        assert!(!sim.entities.is_empty());
        assert!(sim.entities.keys().all(|id| sim.opacity(*id) < 0.5));
        v.live = true;
        v.revision = 4;
        sim.update(&v, Stamp::seconds(202), 13.);
        assert!(sim.events.is_empty());
        assert!(sim.pulses.is_empty());
        sim.update(&View::default(), Stamp::seconds(202), 13.1);
        sim.update(&v, Stamp::seconds(202), 13.2);
        assert!(sim.events.is_empty());
        assert!(sim.pulses.is_empty());
        assert!(callout_opacity(6.) > 0.99);
        assert_eq!(callout_opacity(CALLOUT_DURATION as f32), 0.);
    }
    #[test]
    fn blocked_snapshots_persist_and_each_resolution_reports_the_actual_state() {
        for (status, expected) in [
            ("working", "AGENT WORKING"),
            ("done", "AGENT COMPLETED"),
            ("idle", "AGENT IDLE"),
            ("unrecognized", "AGENT STATE UNKNOWN"),
        ] {
            let mut sim = Simulation::default();
            let blocked = view(
                vec![node("attention", "blocked", 2), node("busy", "working", 1)],
                1,
            );
            sim.update(&blocked, Stamp::seconds(201), 0.);
            let originals: Vec<_> = sim.activities.values().map(|a| a.event.serial).collect();
            sim.update(&blocked, Stamp::seconds(202), 100.);
            assert_eq!(sim.activities.len(), 6);
            assert_eq!(
                sim.activities
                    .values()
                    .map(|a| a.event.serial)
                    .collect::<Vec<_>>(),
                originals
            );
            assert!(sim.activities.values().all(|a| a.persistent));
            let changed = view(
                vec![node("attention", status, 2), node("busy", "working", 1)],
                2,
            );
            sim.update(&changed, Stamp::seconds(202), 101.);
            assert_eq!(
                sim.activities
                    .values()
                    .filter(|a| a.event.title == expected)
                    .count(),
                if status == "working" { 6 } else { 4 }
            );
            sim.update(&changed, Stamp::seconds(202), 112.);
            assert_eq!(
                sim.activities.len(),
                if matches!(status, "working" | "done") {
                    6
                } else {
                    2
                }
            );
        }
    }
    #[test]
    fn blocked_baselines_staleness_and_removal_do_not_invent_live_attention() {
        let mut sim = Simulation::default();
        let mut v = view(vec![node("one", "blocked", 1)], 1);
        sim.update(&v, Stamp::seconds(201), 0.);
        assert_eq!(sim.blocked_nodes().len(), 1);
        sim.update(&v, Stamp::seconds(260), 60.);
        assert!(sim.blocked_nodes().is_empty());
        assert_eq!(sim.activities.len(), 2);
        v.live = false;
        sim.update(&v, Stamp::seconds(260), 100.);
        assert!(sim.blocked_nodes().is_empty());
        let mut baseline = view(vec![node("one", "blocked", 1)], 2);
        baseline.epoch = 2;
        sim.update(&baseline, Stamp::seconds(201), 101.);
        assert_eq!(sim.activities.len(), 2);
        assert!(sim.events.is_empty());
        let mut resolved = view(vec![node("one", "done", 1)], 3);
        resolved.epoch = 3;
        sim.update(&resolved, Stamp::seconds(201), 102.);
        assert_eq!(sim.activities.len(), 2); // Current completed state, not replayed events.
        assert!(
            sim.activities
                .values()
                .all(|a| a.persistent && a.state == AgentState::Completed)
        );
        assert!(sim.events.is_empty());
        assert!(sim.pulses.is_empty());
        let blocked = view(vec![node("one", "blocked", 1)], 4);
        sim.update(&blocked, Stamp::seconds(201), 103.);
        let serial = sim.serial;
        sim.update(&view(vec![], 5), Stamp::seconds(201), 104.);
        assert!(sim.blocked_nodes().is_empty());
        assert!(sim.activities.values().all(|a| !a.persistent
            && a.event.serial > serial
            && a.event.title == "AGENT NO LONGER OBSERVED"));
        sim.update(&view(vec![], 5), Stamp::seconds(201), 115.);
        assert!(sim.activities.is_empty());
    }
    #[test]
    fn offline_node_suppresses_attention_even_with_fresh_agent_inventory() {
        let mut n = node("one", "blocked", 1);
        n.connected = false;
        let mut sim = Simulation::default();
        sim.update(&view(vec![n], 1), Stamp::seconds(201), 2.);
        assert_eq!(sim.activities.len(), 2);
        assert!(sim.blocked_nodes().is_empty());
    }
    #[test]
    fn completed_startup_refresh_reconnect_and_staleness_preserve_current_state() {
        let mut sim = Simulation::default();
        let mut v = view(vec![node("one", "done", 2)], 1);
        sim.update(&v, Stamp::seconds(201), 0.);
        let serials: Vec<_> = sim.activities.values().map(|a| a.event.serial).collect();
        assert_eq!(serials.len(), 4);
        assert!(sim.events.is_empty());
        assert!(sim.pulses.is_empty());
        sim.update(&v, Stamp::seconds(202), 60.);
        assert_eq!(sim.activities.len(), 4);
        let mut renamed = node("one", "done", 2);
        renamed.hostname = "Renamed completed host".into();
        v = view(vec![renamed], 2);
        v.epoch = 2;
        sim.update(&v, Stamp::seconds(202), 61.);
        assert_eq!(
            serials,
            sim.activities
                .values()
                .map(|a| a.event.serial)
                .collect::<Vec<_>>()
        );
        assert!(sim.activities.values().all(|a| a.persistent
            && a.state == AgentState::Completed
            && a.event.title == "AGENT COMPLETED"
            && a.event.text.contains("Renamed completed host")));
        assert!(sim.events.is_empty());
        assert!(sim.pulses.is_empty());
        v.live = false;
        sim.update(&v, Stamp::seconds(260), 120.);
        assert_eq!(sim.activities.len(), 4);
        assert!(sim.activities.keys().all(|key| sim.activity_retained(key)));
        assert!(sim.blocked_nodes().is_empty());
        let mut idle_baseline = view(vec![node("one", "idle", 2)], 3);
        idle_baseline.epoch = 3;
        sim.update(&idle_baseline, Stamp::seconds(202), 121.);
        assert!(sim.activities.is_empty());
        assert!(sim.events.is_empty());
        assert!(sim.pulses.is_empty());
    }
    #[test]
    fn completed_transitions_replace_state_and_only_idle_unknown_are_timed() {
        for (status, title) in [
            ("working", "AGENT WORKING"),
            ("blocked", "ATTENTION REQUIRED"),
            ("idle", "AGENT IDLE"),
            ("unrecognized", "AGENT STATE UNKNOWN"),
        ] {
            let mut sim = Simulation::default();
            sim.update(
                &view(vec![node("one", "done", 1)], 1),
                Stamp::seconds(201),
                0.,
            );
            let serial = sim.serial;
            let changed = view(vec![node("one", status, 1)], 2);
            sim.update(&changed, Stamp::seconds(202), 60.);
            assert_eq!(sim.activities.len(), 2);
            assert!(sim.activities.values().all(|a| a.event.title == title
                && a.event.serial > serial
                && a.persistent == matches!(status, "working" | "blocked")));
            sim.update(&changed, Stamp::seconds(202), 71.);
            assert_eq!(
                sim.activities.len(),
                if matches!(status, "working" | "blocked") {
                    2
                } else {
                    0
                }
            );
        }
        let mut sim = Simulation::default();
        sim.update(
            &view(vec![node("one", "done", 1)], 1),
            Stamp::seconds(201),
            0.,
        );
        sim.update(&view(vec![], 2), Stamp::seconds(202), 60.);
        assert!(
            sim.activities
                .values()
                .all(|a| !a.persistent && a.event.title == "AGENT NO LONGER OBSERVED")
        );
        sim.update(&view(vec![], 2), Stamp::seconds(202), 71.);
        assert!(sim.activities.is_empty());
        sim.update(
            &view(vec![node("one", "done", 1)], 3),
            Stamp::seconds(202),
            72.,
        );
        sim.update(&View::default(), Stamp::seconds(202), 73.);
        assert!(sim.activities.is_empty());
    }
    #[test]
    fn sampled_completed_startup_retention_and_capacity_prioritize_live_attention() {
        let mut sim = Simulation::default();
        let initial = view(
            vec![node("one", "done", 4095), node("two", "done", 4095)],
            1,
        );
        sim.update(&initial, Stamp::seconds(201), 0.);
        assert_eq!(sim.activities.len(), MAX_ENTITIES);
        assert_eq!(sim.omitted_activities, 16380 - MAX_ENTITIES);
        let sampled = sim
            .activities
            .keys()
            .find(|key| sim.id(key).is_none())
            .unwrap()
            .clone();
        assert!(matches!(sim.anchor(&sampled), Some(Id::Node(_))));
        assert!(sim.activities[&sampled].persistent);
        let mixed = view(
            vec![
                node("one", "done", 4095),
                node("two", "done", 4095),
                node("blocked", "blocked", 100),
                node("busy", "working", 100),
            ],
            2,
        );
        sim.update(&mixed, Stamp::seconds(202), 60.);
        assert_eq!(sim.activities.len(), MAX_ENTITIES);
        assert_eq!(
            sim.activities
                .values()
                .filter(|a| a.state == AgentState::Blocked)
                .count(),
            200
        );
        assert_eq!(
            sim.activities
                .values()
                .filter(|a| a.state == AgentState::Working)
                .count(),
            200
        );
        assert_eq!(
            sim.activities
                .values()
                .filter(|a| a.state == AgentState::Completed)
                .count(),
            MAX_ENTITIES - 400
        );
        assert_eq!(sim.omitted_activities, 16780 - MAX_ENTITIES);
        let serials: BTreeMap<_, _> = sim
            .activities
            .iter()
            .map(|(k, a)| (k.clone(), a.event.serial))
            .collect();
        let mut repeat = mixed;
        repeat.revision = 3;
        sim.update(&repeat, Stamp::seconds(202), 61.);
        assert_eq!(serials.len(), sim.activities.len());
        assert!(
            sim.activities
                .iter()
                .all(|(k, a)| serials.get(k) == Some(&a.event.serial)),
            "unchanged capacity must retain card identities without replaying entrances"
        );
        assert!(sim.activities.values().all(|a| a.persistent));
    }
    #[test]
    fn idle_stop_history_survives_sampling_and_yields_bounded_capacity_to_attention() {
        let mut v = view(
            vec![node("one", "idle", 4095), node("two", "idle", 4095)],
            1,
        );
        let keys = v
            .scene
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .flat_map(|n| &n.children)
            .flat_map(|s| &s.children)
            .flat_map(|w| &w.children)
            .filter(|a| a.kind == "agent")
            .map(|a| a.key.clone());
        v.idle_stop_episodes = Arc::new(keys.enumerate().map(|(i, k)| (k, i as u64 + 1)).collect());
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 0.);
        assert_eq!(sim.activities.len(), MAX_ENTITIES);
        assert_eq!(sim.omitted_activities, 16380 - MAX_ENTITIES);
        assert_eq!(sim.summary().states, [0, 0, 0]);
        let sampled = sim
            .activities
            .keys()
            .find(|k| sim.id(k).is_none())
            .unwrap()
            .clone();
        assert!(matches!(sim.anchor(&sampled), Some(Id::Node(_))));
        assert_eq!(sim.activities[&sampled].event.title, "WORK STOPPED — IDLE");
        let serials: BTreeMap<_, _> = sim
            .activities
            .iter()
            .map(|(k, a)| (k.clone(), a.event.serial))
            .collect();
        v.revision += 1;
        sim.update(&v, Stamp::seconds(201), 60.);
        assert!(
            sim.activities
                .iter()
                .all(|(k, a)| a.persistent && serials.get(k) == Some(&a.event.serial))
        );
        let mut mixed = view(
            vec![
                node("one", "idle", 4095),
                node("two", "idle", 4095),
                node("blocked", "blocked", 100),
                node("busy", "working", 100),
            ],
            3,
        );
        mixed.idle_stop_episodes = v.idle_stop_episodes.clone();
        sim.update(&mixed, Stamp::seconds(202), 61.);
        assert_eq!(sim.activities.len(), MAX_ENTITIES);
        assert_eq!(sim.omitted_activities, 16780 - MAX_ENTITIES);
        assert_eq!(sim.summary().states, [200, 200, 0]);
        assert_eq!(
            sim.activities
                .values()
                .filter(|a| a.state == AgentState::Idle && a.persistent)
                .count(),
            MAX_ENTITIES - 400
        );
        assert_eq!(
            sim.activities
                .values()
                .filter(|a| a.state == AgentState::Blocked)
                .count(),
            200
        );
        assert_eq!(
            sim.activities
                .values()
                .filter(|a| a.state == AgentState::Working)
                .count(),
            200
        );
    }
    #[test]
    fn completed_downgrades_release_capacity_before_sampled_blocked_admission() {
        let mut sim = Simulation::default();
        sim.update(
            &view(
                vec![node("one", "blocked", 4095), node("two", "blocked", 4095)],
                1,
            ),
            Stamp::seconds(201),
            0.,
        );
        assert_eq!(sim.activities.len(), MAX_ENTITIES);
        let changed = view(
            vec![node("one", "done", 4095), node("two", "blocked", 4095)],
            2,
        );
        sim.update(&changed, Stamp::seconds(202), 1.);
        assert_eq!(sim.activities.len(), MAX_ENTITIES);
        assert!(
            sim.activities
                .iter()
                .all(|(key, a)| key[1] == "two" && a.persistent && a.state == AgentState::Blocked)
        );
        assert_eq!(sim.omitted_activities, 16380 - MAX_ENTITIES);
    }
    #[test]
    fn agents_without_a_rendered_node_do_not_evict_visible_completed_cards() {
        let mut nodes: Vec<_> = (0..MAX_NODES)
            .map(|n| node(&n.to_string(), "done", 1))
            .collect();
        let mut sim = Simulation::default();
        sim.update(&view(nodes.clone(), 1), Stamp::seconds(201), 0.);
        let removed = nodes.remove(0).instance_id;
        let serials: BTreeMap<_, _> = sim
            .activities
            .iter()
            .filter(|(k, _)| k[1] != removed)
            .map(|(k, a)| (k.clone(), a.event.serial))
            .collect();
        nodes.push(node("replacement", "blocked", 4095));
        let changed = view(nodes, 2);
        // Old node geometry occupies all 128 slots until its exit finishes.
        sim.update(&changed, Stamp::seconds(202), 0.1);
        assert!(sim.id(&vec!["node".into(), "replacement".into()]).is_none());
        assert_eq!(
            sim.activities.values().filter(|a| a.persistent).count(),
            serials.len()
        );
        assert!(
            sim.activities
                .iter()
                .filter(|(_, a)| a.persistent)
                .all(|(k, a)| a.state == AgentState::Completed
                    && serials.get(k) == Some(&a.event.serial))
        );
        assert_eq!(sim.omitted_activities, 8190);
        sim.update(&changed, Stamp::seconds(202), 1.31);
        assert_eq!(sim.activities.len(), MAX_ENTITIES);
        assert!(
            sim.activities
                .values()
                .all(|a| a.state == AgentState::Blocked)
        );
    }
    #[test]
    fn concurrent_work_and_completion_persist_then_idle_notices_expire_independently() {
        let mut sim = Simulation::default();
        let first = view(
            vec![node("one", "working", 4), node("two", "working", 2)],
            1,
        );
        sim.update(&first, Stamp::seconds(201), 0.);
        assert_eq!(sim.activities.len(), 12); // Default and named contexts are distinct.
        let serials: Vec<_> = sim.activities.values().map(|a| a.event.serial).collect();
        sim.update(&first, Stamp::seconds(202), 60.);
        assert!(sim.activities.values().all(|a| a.persistent));
        assert_eq!(
            serials,
            sim.activities
                .values()
                .map(|a| a.event.serial)
                .collect::<Vec<_>>()
        );
        let second = view(vec![node("one", "done", 4), node("two", "working", 2)], 2);
        sim.update(&second, Stamp::seconds(202), 61.);
        assert_eq!(sim.activities.values().filter(|a| a.persistent).count(), 12);
        assert_eq!(
            sim.activities
                .values()
                .filter(|a| a.event.title == "AGENT COMPLETED")
                .count(),
            8
        );
        sim.update(&second, Stamp::seconds(202), 70.9);
        assert_eq!(sim.activities.len(), 12);
        sim.update(&second, Stamp::seconds(202), 71.01);
        assert_eq!(sim.activities.len(), 12);
        let third = view(vec![node("one", "done", 4), node("two", "idle", 2)], 3);
        sim.update(&third, Stamp::seconds(202), 72.);
        assert!(
            sim.activities
                .values()
                .filter(|a| a.state == AgentState::Idle)
                .all(|a| !a.persistent && a.event.title == "AGENT IDLE")
        );
        assert_eq!(sim.activities.values().filter(|a| !a.persistent).count(), 4);
        sim.update(&third, Stamp::seconds(202), 82.01);
        assert_eq!(sim.activities.len(), 8);
        assert!(
            sim.activities
                .values()
                .all(|a| a.persistent && a.state == AgentState::Completed)
        );
    }
    #[test]
    fn working_disconnect_and_reconnect_do_not_invent_completion_or_reuse_anchors() {
        let mut sim = Simulation::default();
        let mut current = view(vec![node("one", "working", 2)], 1);
        sim.update(&current, Stamp::seconds(201), 0.);
        let key = sim.activities.keys().next().unwrap().clone();
        current.live = false;
        sim.update(&current, Stamp::seconds(202), 20.);
        assert_eq!(sim.activities.len(), 4);
        assert!(sim.activities.values().all(|a| a.persistent));
        assert!(sim.opacity(sim.id(&key).unwrap()) < 0.5);
        current = view(vec![node("two", "idle", 2)], 2);
        current.epoch = 2;
        sim.update(&current, Stamp::seconds(202), 21.);
        assert!(sim.activities.is_empty());
        assert!(sim.id(&key).is_none());
        current = view(vec![node("two", "working", 2)], 3);
        current.epoch = 2;
        sim.update(&current, Stamp::seconds(202), 23.);
        current = view(vec![node("three", "idle", 2)], 4);
        current.epoch = 2;
        sim.update(&current, Stamp::seconds(202), 24.);
        assert!(
            sim.activities
                .values()
                .all(|a| !a.persistent && a.event.title == "AGENT NO LONGER OBSERVED")
        );
        assert!(sim.activities.keys().all(|key| sim.id(key).is_none()));
        sim.update(&current, Stamp::seconds(202), 34.01);
        assert!(sim.activities.is_empty());
    }
    #[test]
    fn live_removal_gets_a_new_notice_identity_and_priority() {
        let mut sim = Simulation::default();
        sim.update(
            &view(
                vec![node("gone", "working", 1), node("active", "working", 1)],
                1,
            ),
            Stamp::seconds(201),
            0.,
        );
        sim.update(
            &view(
                vec![node("gone", "working", 1), node("active", "blocked", 1)],
                2,
            ),
            Stamp::seconds(202),
            1.,
        );
        let before = sim.serial;
        sim.update(
            &view(vec![node("active", "blocked", 1)], 3),
            Stamp::seconds(203),
            2.,
        );
        let removed: Vec<_> = sim
            .activities
            .values()
            .filter(|a| a.event.title == "AGENT NO LONGER OBSERVED")
            .collect();
        assert_eq!(removed.len(), 2);
        for notice in removed {
            assert!(!notice.persistent);
            assert_eq!(notice.event.started, 2.);
            assert!(
                notice.event.serial > before,
                "Removal reused old working-card identity and priority"
            );
        }
    }
    #[test]
    fn idle_unknown_stale_and_source_changes_are_honest() {
        let mut sim = Simulation::default();
        sim.update(
            &view(
                vec![node("idle", "idle", 1), node("unknown", "strange", 1)],
                1,
            ),
            Stamp::seconds(201),
            0.,
        );
        assert_eq!(sim.totals.agents, 4);
        assert_eq!(sim.totals.states, [0, 0, 0]);
        assert!(
            sim.records
                .iter()
                .filter(|(id, _)| matches!(id, Id::Agent(..)))
                .all(|(_, r)| matches!(r.state, AgentState::Idle | AgentState::Unknown))
        );
        sim.update(
            &view(
                vec![node("idle", "blocked", 1), node("unknown", "done", 1)],
                2,
            ),
            Stamp::seconds(231),
            1.,
        );
        assert!(sim.events.is_empty());
        assert!(sim.entities.keys().all(|id| sim.opacity(*id) < 1.));
        let mut changed = view(vec![node("other", "working", 1)], 3);
        let mut scene = project(NodeList {
            nodes: vec![node("other", "working", 1)],
        })
        .unwrap();
        scene.coordinator = Some(coordinator(None));
        changed.scene = Some(Arc::new(scene));
        sim.update(&changed, Stamp::seconds(202), 2.);
        assert!(sim.records.values().all(|r| r.names[0].contains("other")));
        assert!(sim.events.is_empty());
    }
    #[test]
    fn bounded_churn_releases_slots_and_retries_without_another_snapshot() {
        let mut sim = Simulation::default();
        for revision in 1..300 {
            let v = view(vec![node(&revision.to_string(), "working", 1)], revision);
            sim.update(&v, Stamp::seconds(201), revision as f64 * 0.002);
            assert!(sim.entities.len() <= MAX_ENTITIES);
            assert!(sim.ids.len() <= MAX_ENTITIES);
            assert!(sim.events.len() <= 3);
            assert!(sim.activities.len() <= MAX_ENTITIES);
        }
        assert!(sim.omitted > 0);
        let v = view(vec![node("299", "working", 1)], 299);
        sim.update(&v, Stamp::seconds(201), 5.);
        assert_eq!(sim.omitted, 0);
        assert_eq!(sim.ids.len(), 7);
        assert!(sim.events.is_empty());
    }
    #[test]
    fn geometry_sampling_cannot_claim_a_still_reported_agent_departed() {
        let mut sim = Simulation::default();
        let first = view(vec![node("one", "working", 3999)], 1);
        sim.update(&first, Stamp::seconds(201), 0.);
        // Adding many ancestor sessions consumes budget previously used by agents.
        let mut changed = node("one", "working", 3999);
        let template = changed.sessions[0].clone();
        changed.sessions.extend((0..63).map(|n| SessionView {
            name: format!("additional-{n}"),
            herdr: None,
            ..template.clone()
        }));
        sim.update(&view(vec![changed], 2), Stamp::seconds(201), 0.1);
        assert!(sim.omitted > 0);
        assert!(sim.activities.values().all(|a| a.persistent));
        assert!(sim.activities.len() <= MAX_ENTITIES);
    }
    #[test]
    fn large_live_detail_and_exits_fit_gpu_buffers_and_keep_complete_counts() {
        let mut sim = Simulation::default();
        let large = view(vec![node("one", "working", 3999)], 1);
        sim.update(&large, Stamp::seconds(201), 0.);
        sim.update(&large, Stamp::seconds(201), 2.);
        assert_eq!(sim.totals.agents, 7998);
        assert!(sim.omitted > 0);
        let replacement = view(vec![node("replacement", "blocked", 3999)], 2);
        sim.update(&replacement, Stamp::seconds(201), 2.1);
        let geometry = crate::mesh_orb::geometry(&sim);
        assert!(geometry.particles.len() <= crate::mesh_orb::MAX_PARTICLES);
        assert!(geometry.lines.len() <= crate::mesh_orb::MAX_LINES);
        assert_eq!(sim.totals.agents, 7998);
        assert!(
            geometry
                .particles
                .iter()
                .all(|p| p.position_size.iter().all(|x| x.is_finite()))
        );
    }
    #[test]
    fn sampled_blocked_cards_resolve_without_departure_and_capacity_is_disclosed() {
        let mut sim = Simulation::default();
        let blocked = view(
            vec![
                node("one", "blocked", 1000),
                node("two", "blocked", 1000),
                node("three", "blocked", 1000),
            ],
            1,
        );
        sim.update(&blocked, Stamp::seconds(201), 0.);
        assert_eq!(sim.activities.len(), 6000);
        assert_eq!(sim.omitted_activities, 0);
        let sampled = sim
            .activities
            .keys()
            .find(|k| k[1] == "three" && sim.id(k).is_none())
            .unwrap()
            .clone();
        assert!(matches!(sim.anchor(&sampled), Some(Id::Node(_))));
        let resolved = view(
            vec![
                node("one", "blocked", 1000),
                node("two", "blocked", 1000),
                node("three", "done", 1000),
            ],
            2,
        );
        sim.update(&resolved, Stamp::seconds(202), 2.);
        assert_eq!(sim.activities[&sampled].event.title, "AGENT COMPLETED");
        assert!(sim.activities[&sampled].persistent);
        sim.update(&resolved, Stamp::seconds(202), 12.01);
        assert_eq!(sim.activities[&sampled].state, AgentState::Completed);
        assert!(sim.activities[&sampled].persistent);
        let huge = view(
            vec![node("one", "blocked", 4095), node("two", "blocked", 4095)],
            3,
        );
        sim.update(&huge, Stamp::seconds(202), 13.);
        assert_eq!(sim.activities.len(), MAX_ENTITIES);
        assert_eq!(sim.omitted_activities, 16380 - MAX_ENTITIES);
        assert!(
            sim.activities
                .values()
                .all(|a| a.persistent && a.state == AgentState::Blocked)
        );
        assert_eq!(sim.blocked_nodes().len(), 2);
    }
    #[test]
    fn every_blocked_node_and_heartbeat_fit_existing_gpu_budgets_together() {
        let mut nodes: Vec<_> = (0..MAX_NODES)
            .map(|n| node(&n.to_string(), "blocked", 10))
            .collect();
        let mut sim = Simulation::default();
        sim.update(&view(nodes.clone(), 1), Stamp::seconds(201), 0.);
        for n in &mut nodes {
            n.last_seen = Some(Timestamp {
                seconds: 201,
                nanos: 0,
            });
        }
        sim.update(&view(nodes, 2), Stamp::seconds(202), 2.);
        assert_eq!(sim.blocked_nodes().len(), MAX_NODES);
        assert_eq!(sim.activities.len(), MAX_NODES * 20);
        assert_eq!(sim.omitted_activities, 0);
        let sampled = sim
            .activities
            .keys()
            .find(|key| sim.id(key).is_none())
            .unwrap();
        assert!(matches!(sim.anchor(sampled), Some(Id::Node(_))));
        assert!(!sim.activity_retained(sampled));
        assert_eq!(sim.pulses.len(), MAX_NODES);
        let g = crate::mesh_orb::geometry(&sim);
        assert!(g.particles.len() <= crate::mesh_orb::MAX_PARTICLES);
        assert!(g.lines.len() <= crate::mesh_orb::MAX_LINES);
        let before = g.particles.len();
        sim.live = false;
        assert!(crate::mesh_orb::geometry(&sim).particles.len() < before);
    }
    #[test]
    fn blocked_circuits_leave_headroom_for_dense_repeated_session_exits() {
        let nodes: Vec<_> = (0..MAX_NODES)
            .map(|n| {
                let mut n = node(&n.to_string(), "blocked", 1);
                let session = n.sessions[0].clone();
                n.sessions = (0..8)
                    .map(|s| SessionView {
                        name: format!("s{s}"),
                        ..session.clone()
                    })
                    .collect();
                n
            })
            .collect();
        let mut sim = Simulation::default();
        sim.update(&view(nodes.clone(), 1), Stamp::seconds(202), 0.);
        sim.update(&view(nodes.clone(), 1), Stamp::seconds(202), 1.3);
        for rev in 2..=5 {
            let mut changed = nodes.clone();
            for n in &mut changed {
                n.last_seen = Some(Timestamp {
                    seconds: 201,
                    nanos: 0,
                });
                for s in &mut n.sessions {
                    s.incarnation = format!("epoch-{rev}");
                }
            }
            sim.update(
                &view(changed, rev),
                Stamp::seconds(202),
                1.2 + rev as f64 * 0.1,
            );
            assert_eq!(sim.blocked_nodes().len(), MAX_NODES);
            assert!(sim.entities.values().any(|l| !l.entering()));
            let g = crate::mesh_orb::geometry(&sim);
            assert!(g.lines.len() <= crate::mesh_orb::MAX_LINES);
            assert!(g.particles.len() <= crate::mesh_orb::MAX_PARTICLES);
        }
    }
    #[test]
    fn every_node_hub_survives_dense_detail_and_simultaneous_receipt_effects() {
        let nodes: Vec<_> = (0..MAX_NODES)
            .map(|n| {
                let mut node = node(&n.to_string(), "working", 0);
                let session = node.sessions[0].clone();
                node.sessions = (0..64)
                    .map(|s| SessionView {
                        name: format!("session-{s}"),
                        ..session.clone()
                    })
                    .collect();
                node
            })
            .collect();
        let mut sim = Simulation::default();
        sim.update(&view(nodes.clone(), 1), Stamp::seconds(201), 0.);
        let changed = nodes
            .into_iter()
            .map(|mut node| {
                node.last_seen = Some(Timestamp {
                    seconds: 201,
                    nanos: 0,
                });
                node
            })
            .collect();
        sim.update(&view(changed, 2), Stamp::seconds(202), 2.);
        assert_eq!(
            sim.entities
                .iter()
                .filter(|(id, life)| matches!(id, Id::Node(_)) && life.entering())
                .count(),
            MAX_NODES
        );
        assert_eq!(sim.pulses.len(), MAX_NODES);
        assert!(sim.omitted > 0);
        assert_eq!(sim.summary().sessions, MAX_NODES * 65);
        let geometry = crate::mesh_orb::geometry(&sim);
        assert!(geometry.particles.len() <= crate::mesh_orb::MAX_PARTICLES);
        assert!(geometry.lines.len() <= crate::mesh_orb::MAX_LINES);
    }
}
