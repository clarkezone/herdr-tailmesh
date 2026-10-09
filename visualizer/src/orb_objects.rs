//! Scoped, read-only typed callouts. Presentation toggles never acknowledge outcomes.
use crate::{
    mesh_model::{AgentState, Event, Id, Simulation},
    mesh_orb::Glyph,
};
use herdr_mesh_visualizer::{
    client::View,
    projection::{Branch, Key, wall_now},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Node,
    Session,
    Workspace,
}
impl Class {
    fn index(self) -> usize {
        match self {
            Self::Node => 0,
            Self::Session => 1,
            Self::Workspace => 2,
        }
    }
}
pub struct Object {
    pub key: Key,
    pub event: Event,
    pub glyph: Glyph,
}
#[derive(Default)]
pub struct Objects {
    enabled: [bool; 3],
    suppressed: BTreeSet<Key>,
    selected: Option<Key>,
    revision: Option<(u64, u64, bool, u64, i64)>,
    dirty: bool,
    pub cards: BTreeMap<Key, Arc<Object>>,
}
impl Objects {
    pub fn toggle(&mut self, class: Class) {
        self.enabled[class.index()] = !self.enabled[class.index()];
        self.suppressed.retain(|key| {
            key.len()
                != match class {
                    Class::Node => 2,
                    Class::Session => 5,
                    Class::Workspace => 7,
                }
        });
        self.dirty = true;
    }
    pub fn close(&mut self, key: &Key) {
        self.suppressed.insert(key.clone());
        self.dirty = true;
    }
    pub fn update(&mut self, sim: &Simulation, view: &View, selected: Option<&Key>) {
        if self.selected.as_ref() != selected {
            if let Some(key) = selected {
                self.suppressed.remove(key);
            }
            self.selected = selected.cloned();
            self.dirty = true;
        }
        let revision = (
            view.epoch,
            view.revision,
            view.live,
            sim.source_generation,
            wall_now().seconds,
        );
        if self.revision == Some(revision) && !self.dirty {
            return;
        }
        self.revision = Some(revision);
        self.dirty = false;
        let mut desired = BTreeMap::new();
        let mut present = BTreeSet::new();
        if let Some(scene) = &view.scene {
            let mut path = Vec::new();
            if let Some(root) = &scene.coordinator {
                self.scan(root, &mut path, sim, view, &mut desired, &mut present);
            }
            for node in &scene.nodes {
                self.scan(node, &mut path, sim, view, &mut desired, &mut present);
            }
        }
        self.suppressed.retain(|key| present.contains(key));
        self.cards = desired;
    }
    fn scan(
        &self,
        branch: &Branch,
        path: &mut Vec<String>,
        sim: &Simulation,
        view: &View,
        cards: &mut BTreeMap<Key, Arc<Object>>,
        present: &mut BTreeSet<Key>,
    ) {
        let id = sim
            .id(&branch.key)
            .filter(|id| sim.entities.get(id).is_some_and(|life| life.entering()));
        // The dedicated coordinator representation has no execution Id.
        let represented = id.is_some() || branch.kind == "coordinator";
        if represented {
            present.insert(branch.key.clone());
        }
        if !represented {
            return;
        }
        let bulk = match branch.kind {
            "node" => self.enabled[0],
            "session" => self.enabled[1],
            "workspace" => self.enabled[2],
            _ => false,
        };
        let selected = self.selected.as_ref() == Some(&branch.key);
        if represented && (bulk || selected) && !self.suppressed.contains(&branch.key) {
            let glyph = match branch.kind {
                "coordinator" => Glyph::Coordinator,
                "node" => Glyph::Node,
                "session" => Glyph::Session,
                "workspace" => Glyph::Workspace,
                _ => Glyph::Agent(match branch.status.as_str() {
                    "working" => AgentState::Working,
                    "blocked" => AgentState::Blocked,
                    "done" => AgentState::Completed,
                    "idle" => AgentState::Idle,
                    _ => AgentState::Unknown,
                }),
            };
            let mut lines = path.clone();
            lines.push(format!("{}: {}", branch.kind, branch.label));
            lines.push(format!(
                "State: {} · {}",
                if branch.status.is_empty() {
                    "not reported"
                } else {
                    &branch.status
                },
                if view.live && branch.kind == "coordinator" {
                    "control connection live".into()
                } else if view.live {
                    branch.freshness.label_at(wall_now()).to_string()
                } else {
                    "LAST KNOWN · disconnected".into()
                }
            ));
            if let Some(stamp) = branch.last_seen {
                lines.push(format!(
                    "Last seen: {}",
                    if stamp <= wall_now() {
                        format!(
                            "{}s ago",
                            (wall_now().nanos() - stamp.nanos()) / 1_000_000_000
                        )
                    } else {
                        "unknown (clock ahead)".into()
                    }
                ));
            }
            if !branch.children.is_empty() {
                lines.push(format!(
                    "{} {}",
                    branch.children.len(),
                    match branch.kind {
                        "node" => "sessions",
                        "session" => "workspaces",
                        "workspace" => "agents",
                        _ => "children",
                    }
                ));
            }
            lines.extend(branch.details.iter().cloned());
            let text = lines.join("\n");
            let title = match branch.kind {
                "coordinator" => "COORDINATOR",
                "node" => "NODE",
                "session" => "SESSION",
                "workspace" => "WORKSPACE",
                _ => "AGENT",
            };
            let old = self.cards.get(&branch.key).filter(|o| o.event.text == text);
            let card = old.cloned().unwrap_or_else(|| {
                Arc::new(Object {
                    key: branch.key.clone(),
                    glyph,
                    event: Event {
                        serial: 0,
                        origin: id.unwrap_or(Id::Node(0)),
                        started: sim.clock,
                        title,
                        text,
                        color: glyph.color(),
                    },
                })
            });
            cards.insert(branch.key.clone(), card);
        }
        path.push(format!("{}: {}", branch.kind, branch.label));
        for child in &branch.children {
            self.scan(child, path, sim, view, cards, present);
        }
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_model::tests::{node, view};
    use herdr_mesh_visualizer::heartbeat::Stamp;
    #[test]
    fn independent_bulk_toggles_deduplicate_selection_and_close_only_exact_scope() {
        let v = view(
            vec![node("one", "working", 1), node("two", "working", 1)],
            1,
        );
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 2.);
        let mut objects = Objects::default();
        objects.update(&sim, &v, None);
        assert!(objects.cards.is_empty());
        for class in [Class::Node, Class::Session, Class::Workspace] {
            objects.toggle(class);
        }
        objects.update(&sim, &v, None);
        assert_eq!(objects.cards.len(), 10);
        let workspace = objects.cards.keys().find(|k| k.len() == 7).unwrap().clone();
        objects.update(&sim, &v, Some(&workspace));
        assert_eq!(objects.cards.len(), 10);
        objects.close(&workspace);
        objects.update(&sim, &v, Some(&workspace));
        assert_eq!(objects.cards.len(), 9);
        assert!(!objects.cards.contains_key(&workspace));
        objects.toggle(Class::Node);
        objects.toggle(Class::Node);
        objects.update(&sim, &v, None);
        assert!(
            !objects.cards.contains_key(&workspace),
            "Node toggle must not clear workspace suppression"
        );
        objects.update(&sim, &v, Some(&workspace));
        assert_eq!(objects.cards.len(), 10);
        objects.close(&workspace);
        objects.update(&sim, &v, None);
        objects.toggle(Class::Workspace);
        objects.toggle(Class::Workspace);
        objects.update(&sim, &v, None);
        assert_eq!(objects.cards.len(), 10);
        assert_eq!(sim.summary().agents, 4);
        assert_eq!(sim.summary().states[0], 4);
    }
    #[test]
    fn typed_details_glyphs_and_versions_are_real_not_coordinator_inferences() {
        let mut n = node("one", "done", 1);
        n.implementation_version = "mesh-member-v2".into();
        n.herdr.as_mut().unwrap().version = "herdr-default-v1".into();
        n.sessions[0].herdr.as_mut().unwrap().version = "herdr-named-v3".into();
        let v = view(vec![n], 1);
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 2.);
        let mut objects = Objects::default();
        objects.toggle(Class::Node);
        objects.update(&sim, &v, None);
        let node = objects.cards.values().next().unwrap();
        assert!(matches!(node.glyph, Glyph::Node));
        for part in [
            "Actual node one",
            "mesh-member-v2",
            "herdr-default-v1",
            "herdr-named-v3",
        ] {
            assert!(node.event.text.contains(part));
        }
        let root = v.scene.as_ref().unwrap().coordinator.as_ref().unwrap();
        objects.update(&sim, &v, Some(&root.key));
        assert!(matches!(objects.cards[&root.key].glyph, Glyph::Coordinator));
        let agent = v.scene.as_ref().unwrap().nodes[0].children[0].children[0].children[0]
            .key
            .clone();
        objects.update(&sim, &v, Some(&agent));
        let selected = &objects.cards[&agent];
        assert!(matches!(
            selected.glyph,
            Glyph::Agent(AgentState::Completed)
        ));
        for part in ["session:", "workspace:", "agent:", "State: done"] {
            assert!(selected.event.text.contains(part));
        }
        let old = view(vec![crate::mesh_model::tests::node("one", "idle", 1)], 2);
        sim.update(&old, Stamp::seconds(201), 3.);
        objects.update(&sim, &old, None);
        assert!(
            objects
                .cards
                .values()
                .any(|o| o.event.text.contains("herdr-mesh version: unknown"))
        );
    }
    #[test]
    fn new_incarnation_removal_and_sampled_entities_cannot_inherit_callouts() {
        let mut n = node("one", "idle", 3999);
        let v = view(vec![n.clone()], 1);
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 0.);
        let omitted = v.scene.as_ref().unwrap().nodes[0]
            .children
            .iter()
            .flat_map(|s| &s.children)
            .flat_map(|w| &w.children)
            .find(|a| sim.id(&a.key).is_none())
            .unwrap()
            .key
            .clone();
        let mut objects = Objects::default();
        objects.update(&sim, &v, Some(&omitted));
        assert!(objects.cards.is_empty());
        objects.toggle(Class::Session);
        objects.update(&sim, &v, None);
        let old = objects
            .cards
            .keys()
            .find(|k| !k[3].is_empty())
            .unwrap()
            .clone();
        objects.close(&old);
        n.sessions[0].incarnation = "two".into();
        let next = view(vec![n], 2);
        sim.update(&next, Stamp::seconds(201), 2.);
        objects.update(&sim, &next, None);
        assert!(!objects.cards.contains_key(&old));
        assert!(objects.cards.keys().any(|k| k[4] == "two"));
        let gone = view(vec![], 3);
        sim.update(&gone, Stamp::seconds(201), 3.);
        objects.update(&sim, &gone, None);
        assert!(objects.cards.is_empty());
        assert!(objects.suppressed.is_empty());
    }
}
