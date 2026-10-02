//! Bounded receipt effects: replacement snapshots are not an event log.
use crate::projection::{Key, Scene};
use prost_types::Timestamp;
use std::collections::HashMap;

pub const PULSE_DURATION: f64 = 3.0;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp {
    pub seconds: i64,
    pub nanos: i32,
}
impl Stamp {
    pub fn seconds(seconds: i64) -> Self {
        Self { seconds, nanos: 0 }
    }
    pub fn parse(t: &Timestamp) -> Option<Self> {
        ((0..1_000_000_000).contains(&t.nanos) && (0..=253_402_300_799).contains(&t.seconds))
            .then_some(Self {
                seconds: t.seconds,
                nanos: t.nanos,
            })
    }
    pub fn nanos(self) -> i128 {
        i128::from(self.seconds) * 1_000_000_000 + i128::from(self.nanos)
    }
}
#[derive(Debug)]
pub struct Heartbeat {
    pub key: Key,
    pub connected: bool,
    pub seen: Option<Stamp>,
}
#[derive(Default)]
pub struct Pulses {
    epoch: Option<u64>,
    revision: Option<u64>,
    source: Option<Key>,
    baselines: HashMap<Key, Stamp>,
    active: HashMap<Key, f64>,
}
impl Pulses {
    pub fn update(
        &mut self,
        scene: &Scene,
        live: bool,
        epoch: u64,
        revision: u64,
        wall: Stamp,
        clock: f64,
    ) {
        let source = scene.coordinator.as_ref().map(|b| b.key.clone());
        if !live {
            self.baselines.clear();
            self.active.clear();
            self.revision = None;
            return;
        }
        if self.epoch != Some(epoch) || self.source != source {
            self.baselines.clear();
            self.active.clear();
            self.revision = None;
        }
        self.epoch = Some(epoch);
        self.source = source;
        if self.revision != Some(revision) {
            let mut next = HashMap::new();
            for h in &scene.heartbeats {
                let Some(seen) = h.seen.filter(|t| h.connected && *t <= wall) else {
                    self.active.remove(&h.key);
                    continue;
                };
                if self
                    .baselines
                    .get(&h.key)
                    .is_some_and(|previous| seen > *previous)
                {
                    self.active.insert(h.key.clone(), clock);
                }
                next.insert(h.key.clone(), seen);
            }
            self.baselines = next;
            self.active
                .retain(|key, _| self.baselines.contains_key(key));
            self.revision = Some(revision);
        }
        self.active
            .retain(|_, start| clock >= *start && clock - *start < PULSE_DURATION);
    }
    pub fn iter(&self, clock: f64) -> impl Iterator<Item = (&Key, f32)> {
        self.active
            .iter()
            .map(move |(key, start)| (key, ((clock - start) / PULSE_DURATION).clamp(0., 1.) as f32))
    }
    pub fn is_active(&self) -> bool {
        !self.active.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene(stamp: Option<Stamp>, connected: bool) -> Scene {
        Scene {
            heartbeats: vec![Heartbeat {
                key: vec!["node".into()],
                connected,
                seen: stamp,
            }],
            ..Default::default()
        }
    }
    fn update(
        p: &mut Pulses,
        stamp: Option<Stamp>,
        connected: bool,
        epoch: u64,
        revision: u64,
        clock: f64,
    ) {
        p.update(
            &scene(stamp, connected),
            true,
            epoch,
            revision,
            Stamp::seconds(200),
            clock,
        );
    }
    #[test]
    fn nanosecond_advances_duplicates_and_reconnect_coalescing() {
        let mut p = Pulses::default();
        let a = Stamp {
            seconds: 100,
            nanos: 1,
        };
        let b = Stamp {
            seconds: 100,
            nanos: 2,
        };
        update(&mut p, Some(a), true, 1, 1, 0.);
        assert!(!p.is_active());
        update(&mut p, Some(b), true, 1, 2, 1.);
        assert_eq!(p.iter(1.).count(), 1);
        let quarter = 1. + PULSE_DURATION * 0.25;
        update(&mut p, Some(b), true, 1, 3, quarter);
        assert!((p.iter(quarter).next().unwrap().1 - 0.25).abs() < 0.001);
        // The UI need not render false-live between these accepted watches.
        update(&mut p, Some(Stamp::seconds(110)), true, 2, 4, quarter + 0.1);
        assert!(!p.is_active());
        let start = quarter + 0.2;
        update(&mut p, Some(Stamp::seconds(111)), true, 2, 5, start);
        assert!(p.is_active());
        update(
            &mut p,
            Some(Stamp::seconds(111)),
            true,
            2,
            5,
            start + PULSE_DURATION - 0.001,
        );
        assert!(p.is_active());
        update(
            &mut p,
            Some(Stamp::seconds(111)),
            true,
            2,
            5,
            start + PULSE_DURATION + 0.001,
        );
        assert!(!p.is_active());
    }
    #[test]
    fn invalid_future_regressed_disconnected_and_removed_receipts_do_not_replay() {
        assert!(
            Stamp::parse(&Timestamp {
                seconds: i64::MAX,
                nanos: 0
            })
            .is_none()
        );
        assert!(
            Stamp::parse(&Timestamp {
                seconds: 100,
                nanos: -1
            })
            .is_none()
        );
        let mut p = Pulses::default();
        update(&mut p, Some(Stamp::seconds(100)), true, 1, 1, 0.);
        update(&mut p, Some(Stamp::seconds(99)), true, 1, 2, 0.1);
        assert!(!p.is_active());
        update(&mut p, Some(Stamp::seconds(201)), true, 1, 3, 0.2);
        assert!(!p.is_active());
        update(&mut p, Some(Stamp::seconds(101)), true, 1, 4, 0.3);
        assert!(!p.is_active());
        update(&mut p, Some(Stamp::seconds(102)), true, 1, 5, 0.4);
        assert!(p.is_active());
        update(&mut p, Some(Stamp::seconds(103)), false, 1, 6, 0.5);
        assert!(!p.is_active());
        update(&mut p, Some(Stamp::seconds(104)), true, 1, 7, 0.6);
        assert!(!p.is_active());
        update(&mut p, Some(Stamp::seconds(105)), true, 1, 8, 0.7);
        assert!(p.is_active());
        p.update(&Scene::default(), true, 1, 9, Stamp::seconds(200), 0.8);
        assert!(!p.is_active());
        assert!(p.baselines.is_empty());
    }
    #[test]
    fn disconnect_and_source_change_clear_bounded_effects() {
        let mut p = Pulses::default();
        update(&mut p, Some(Stamp::seconds(100)), true, 1, 1, 0.);
        update(&mut p, Some(Stamp::seconds(101)), true, 1, 2, 0.1);
        p.update(
            &scene(Some(Stamp::seconds(101)), true),
            false,
            1,
            2,
            Stamp::seconds(200),
            0.2,
        );
        assert!(!p.is_active());
        update(&mut p, Some(Stamp::seconds(102)), true, 1, 3, 0.3);
        assert!(!p.is_active());
        let mut changed = scene(Some(Stamp::seconds(103)), true);
        changed.coordinator = Some(crate::projection::coordinator(Some(
            &crate::pb::ServerInfo {
                instance_id: "different".into(),
                ..Default::default()
            },
        )));
        p.update(&changed, true, 1, 4, Stamp::seconds(200), 0.4);
        assert!(!p.is_active());
        assert!(p.baselines.len() <= changed.nodes.len() + changed.heartbeats.len());
    }
}
