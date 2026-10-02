//! Dashboard-equivalent counts, independent of layout and synthetic branches.
use crate::{
    heartbeat::Stamp,
    projection::{Freshness, Scene},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub connected: usize,
    pub fresh: usize,
    pub workspaces: usize,
    pub agents: usize,
    pub working: usize,
    pub blocked: usize,
    pub done: usize,
}
impl Counts {
    pub fn values(self) -> [usize; 7] {
        [
            self.connected,
            self.fresh,
            self.workspaces,
            self.agents,
            self.working,
            self.blocked,
            self.done,
        ]
    }
    fn add_inventory(&mut self, other: Self) {
        self.workspaces += other.workspaces;
        self.agents += other.agents;
        self.working += other.working;
        self.blocked += other.blocked;
        self.done += other.done;
    }
}
#[derive(Debug)]
pub struct ContextCounts {
    pub freshness: Freshness,
    pub inventory: Counts,
}
#[derive(Debug)]
pub struct NodeCounts {
    pub connected: bool,
    pub contexts: Vec<ContextCounts>,
}
#[derive(Debug, Default)]
pub struct Summary {
    pub known: Counts,
    pub fresh: Counts,
    pub total: usize,
}
pub fn summary(scene: &Scene, time: Stamp) -> Summary {
    let mut result = Summary {
        total: scene.counts.len(),
        ..Default::default()
    };
    for node in &scene.counts {
        result.known.connected += usize::from(node.connected);
        result.fresh.connected += usize::from(node.connected);
        let mut fresh = false;
        for context in &node.contexts {
            result.known.add_inventory(context.inventory);
            if node.connected && context.freshness.is_live(time) {
                result.fresh.add_inventory(context.inventory);
                fresh = true;
            }
        }
        result.fresh.fresh += usize::from(fresh);
    }
    result.known.fresh = summary_fresh_at_snapshot(scene);
    result
}
fn summary_fresh_at_snapshot(scene: &Scene) -> usize {
    scene
        .counts
        .iter()
        .filter(|n| {
            n.connected
                && n.contexts
                    .iter()
                    .any(|c| c.freshness.is_live(scene.observed_at))
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        pb::{HerdrEntity, HerdrState, NodeList, NodeView, SessionView},
        projection::project,
    };
    use prost_types::Timestamp;
    fn state() -> HerdrState {
        HerdrState {
            status: "ready".into(),
            workspaces: ["w", "w2"]
                .into_iter()
                .map(|id| HerdrEntity {
                    id: id.into(),
                    ..Default::default()
                })
                .collect(),
            agents: ["working", "blocked", "done", "idle", "unrecognized"]
                .into_iter()
                .enumerate()
                .map(|(i, s)| HerdrEntity {
                    id: format!("a{i}"),
                    workspace_id: ["w", "w2", "absent", "w", "w2"][i].into(),
                    agent_status: s.into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }
    fn fixture() -> Scene {
        let ts = Some(Timestamp {
            seconds: 100,
            nanos: 500_000_000,
        });
        let node = |id: &str, connected: bool| NodeView {
            instance_id: id.into(),
            connected,
            herdr: Some(state()),
            herdr_received_at: ts,
            sessions_ready: true,
            sessions: vec![SessionView {
                name: "native".into(),
                incarnation: "one".into(),
                status: "ready".into(),
                herdr: Some(state()),
                herdr_received_at: ts,
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut scene = project(NodeList {
            nodes: vec![node("one", true), node("two", false)],
        })
        .unwrap();
        scene.observed_at = Stamp::seconds(101);
        scene
    }
    #[test]
    fn scoped_counts_exclude_orphan_placeholders_and_coordinator() {
        let mut scene = fixture();
        scene.coordinator = Some(crate::projection::coordinator(None));
        let c = summary(&scene, Stamp::seconds(101));
        assert_eq!(c.total, 2);
        assert_eq!(c.fresh.values(), [1, 1, 4, 10, 2, 2, 2]);
        assert_eq!(c.known.values(), [1, 1, 8, 20, 4, 4, 4]);
        assert!(c.fresh.agents > c.fresh.working + c.fresh.blocked + c.fresh.done);
        // Each context includes an unresolved workspace for its orphan agents.
        assert_eq!(scene.nodes[0].children[0].children.len(), 3);
    }
    #[test]
    fn full_precision_aging_and_clock_skew_match_dashboard_threshold() {
        let scene = fixture();
        assert_eq!(
            summary(
                &scene,
                Stamp {
                    seconds: 130,
                    nanos: 499_999_999
                }
            )
            .fresh
            .fresh,
            1
        );
        assert_eq!(
            summary(
                &scene,
                Stamp {
                    seconds: 130,
                    nanos: 500_000_000
                }
            )
            .fresh
            .values(),
            [1, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            summary(
                &scene,
                Stamp {
                    seconds: 70,
                    nanos: 500_000_000
                }
            )
            .fresh
            .fresh,
            0
        );
        assert_eq!(
            summary(
                &scene,
                Stamp {
                    seconds: 70,
                    nanos: 500_000_001
                }
            )
            .fresh
            .fresh,
            1
        );
        assert_eq!(
            summary(&scene, Stamp::seconds(140)).known.fresh,
            1,
            "retained snapshot-time freshness does not claim current freshness"
        );
    }
    #[test]
    fn stopped_disabled_stale_unknown_and_independent_context_receipts() {
        let make = |status: &str, stale: bool, received: Option<Timestamp>| {
            project(NodeList {
                nodes: vec![NodeView {
                    instance_id: "node".into(),
                    connected: true,
                    herdr: Some(HerdrState {
                        status: "disabled".into(),
                        ..Default::default()
                    }),
                    sessions_ready: true,
                    sessions_received_at: Some(Timestamp {
                        seconds: 100,
                        nanos: 0,
                    }),
                    sessions: vec![SessionView {
                        name: "native".into(),
                        status: status.into(),
                        stale,
                        herdr: Some(state()),
                        herdr_received_at: received,
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
            })
            .unwrap()
        };
        let ready = make(
            "ready",
            false,
            Some(Timestamp {
                seconds: 100,
                nanos: 0,
            }),
        );
        assert_eq!(ready.counts[0].contexts.len(), 1);
        assert_eq!(
            summary(&ready, Stamp::seconds(101)).fresh.values(),
            [1, 1, 2, 5, 1, 1, 1]
        );
        for scene in [
            // A current aggregate discovery receipt must not refresh inventory.
            make("ready", false, None),
            make("stopped", false, None),
            make("ready", true, None),
            make(
                "ready",
                false,
                Some(Timestamp {
                    seconds: i64::MAX,
                    nanos: 0,
                }),
            ),
        ] {
            assert_eq!(
                summary(&scene, Stamp::seconds(101)).fresh.values(),
                [1, 0, 0, 0, 0, 0, 0]
            );
            let retained = summary(&scene, Stamp::seconds(101)).known;
            assert_eq!(retained.workspaces, 2);
            assert_eq!(retained.agents, 5);
        }
        assert_eq!(
            summary(&Scene::default(), Stamp::seconds(100))
                .fresh
                .values(),
            [0; 7]
        );
    }
}
