//! Presentation state derived exclusively from the production observation model.
use herdr_mesh_visualizer::{
    client::View,
    heartbeat::{Pulses, Stamp},
    projection::{Branch, Freshness, Key, Scene},
    summary::summary,
};
use std::collections::{BTreeMap, HashSet, VecDeque};

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
    pub event: Event,
    pub persistent: bool,
}
#[derive(Default)]
pub struct Simulation {
    pub entities: BTreeMap<Id, Life>,
    records: BTreeMap<Id, Record>,
    ids: BTreeMap<Key, Id>,
    pub pulses: Vec<Pulse>,
    pub events: VecDeque<Event>,
    /// Current working agents and their timed stop notices, scoped by identity.
    pub activities: BTreeMap<Key, Activity>,
    pub time: f32,
    pub clock: f64,
    pub live: bool,
    pub callouts: bool,
    pub omitted: usize,
    totals: Summary,
    pub source_generation: u64,
    source: Option<Key>,
    epoch: Option<u64>,
    revision: Option<u64>,
    receipts: Pulses,
    serial: u64,
    wall: Stamp,
}
impl Simulation {
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
    fn prune(&mut self) -> bool {
        let previous = self.entities.len();
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
        previous != self.entities.len()
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
    fn activity(&mut self, key: &Key, id: Id, state: AgentState, text: String) {
        let persistent = state == AgentState::Working;
        if let Some(activity) = self.activities.get_mut(key)
            && activity.persistent
            && persistent
        {
            activity.event.text = text;
            return;
        }
        self.serial = self.serial.wrapping_add(1);
        self.activities.insert(
            key.clone(),
            Activity {
                persistent,
                event: Event {
                    serial: self.serial,
                    origin: id,
                    started: self.clock,
                    title: match state {
                        AgentState::Working => "AGENT WORKING",
                        AgentState::Completed => "AGENT COMPLETED",
                        AgentState::Blocked => "ATTENTION REQUIRED",
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
        self.callouts = true;
        let freed = self.prune();
        let Some(scene) = &view.scene else {
            self.live = false;
            self.pulses.clear();
            self.events.clear();
            self.activities.clear();
            self.receipts = Default::default();
            self.revision = None;
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
        if self.revision != Some(view.revision) || freed && self.omitted > 0 {
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
                let working_card = self
                    .activities
                    .get(&branch.key)
                    .is_some_and(|a| a.persistent);
                if branch.kind == "agent" && state == AgentState::Working
                    || attention && self.activities.contains_key(&branch.key)
                {
                    let text = format!(
                        "{}\nNode: {}\nSession: {}\nWorkspace: {}\nAgent: {}",
                        branch.status, names[0], names[1], names[2], names[3]
                    );
                    self.activity(&branch.key, id, state, text);
                } else if attention {
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
                } else if branch.kind == "agent" && state != AgentState::Working && working_card {
                    // Stale/reconnected comparisons cannot fabricate completion.
                    self.activities.remove(&branch.key);
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
                a.persistent && !self.ids.get(*key).is_some_and(|id| selected.contains(id))
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
            if activity.persistent && !self.ids.get(key).is_some_and(|id| selected.contains(id)) {
                if !emit || present.contains(key) {
                    return false; // Baselines and render sampling cannot assert departure.
                }
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
        // Trim completed history once per snapshot, avoiding repeated full-map scans.
        let excess = self.activities.len().saturating_sub(MAX_ENTITIES);
        if excess > 0 {
            let mut timed: Vec<_> = self
                .activities
                .iter()
                .filter(|(_, a)| !a.persistent)
                .map(|(key, a)| (a.event.serial, key.clone()))
                .collect();
            timed.sort_unstable_by_key(|(serial, _)| *serial);
            for (_, key) in timed.into_iter().take(excess) {
                self.activities.remove(&key);
            }
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
        assert!(sim.activities.is_empty());
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
    fn concurrent_work_persists_then_each_actual_stop_expires_independently() {
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
        assert_eq!(sim.activities.values().filter(|a| a.persistent).count(), 4);
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
        assert_eq!(sim.activities.len(), 4);
        let third = view(vec![node("one", "done", 4), node("two", "idle", 2)], 3);
        sim.update(&third, Stamp::seconds(202), 72.);
        assert!(
            sim.activities
                .values()
                .all(|a| !a.persistent && a.event.title == "AGENT IDLE")
        );
        sim.update(&third, Stamp::seconds(202), 82.01);
        assert!(sim.activities.is_empty());
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
