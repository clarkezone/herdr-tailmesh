//! Latest observed agent outcomes, independent of current agent status.
use crate::projection::{Branch, Key, Scene};
use prost::Message;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    path::PathBuf,
};

pub const MAX_OUTCOMES: usize = 8000;
const MAX_BYTES: u64 = 16 * 1024 * 1024;
type Scope = (Key, Key);

#[derive(Clone, PartialEq, Message)]
pub struct Outcome {
    #[prost(string, repeated, tag = "1")]
    pub source: Key,
    #[prost(string, repeated, tag = "2")]
    pub agent: Key,
    #[prost(uint64, tag = "3")]
    pub episode: u64,
    #[prost(string, tag = "4")]
    pub kind: String,
    #[prost(string, repeated, tag = "5")]
    pub names: Vec<String>,
    #[prost(bool, tag = "6")]
    pub dismissed: bool,
    #[prost(int64, tag = "7")]
    pub observed_seconds: i64,
    #[prost(int32, tag = "8")]
    pub observed_nanos: i32,
}
impl Outcome {
    pub fn card_key(&self) -> Key {
        let mut key = self.agent.clone();
        key.push("outcome".into());
        key
    }
}
#[derive(Clone, PartialEq, Message)]
struct Saved {
    #[prost(uint32, tag = "1")]
    version: u32,
    #[prost(message, repeated, tag = "2")]
    records: Vec<Outcome>,
}
#[derive(Clone)]
struct Change {
    remove: bool,
    new_after_failed_load: bool,
    expected: Option<u64>,
    outcome: Outcome,
}
#[derive(Clone)]
pub(crate) struct OutcomeStore {
    path: PathBuf,
}
impl OutcomeStore {
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
    fn transaction(
        &self,
        changes: &BTreeMap<Scope, Change>,
    ) -> io::Result<BTreeMap<Scope, Outcome>> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("Missing outcome directory"))?;
        fs::create_dir_all(parent)?;
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.path.with_extension("lock"))?;
        lock.try_lock()?;
        let mut file_present = true;
        let bytes = match File::open(&self.path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
                bytes
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                file_present = false;
                Vec::new()
            }
            Err(e) => return Err(e),
        };
        if bytes.len() as u64 > MAX_BYTES {
            return Err(io::Error::other("Outcome journal exceeds its size limit"));
        }
        let saved = if !file_present {
            Saved {
                version: 1,
                records: vec![],
            }
        } else {
            Saved::decode(bytes.as_slice())
                .map_err(|_| io::Error::other("Invalid outcome journal"))?
        };
        let mut records = BTreeMap::new();
        if file_present && bytes.is_empty() {
            return Err(io::Error::other("Empty outcome journal"));
        }
        if saved.version != 1 || saved.records.len() > MAX_OUTCOMES {
            return Err(io::Error::other("Unsupported outcome journal"));
        }
        for r in saved.records {
            if !valid(&r)
                || records
                    .insert((r.source.clone(), r.agent.clone()), r)
                    .is_some()
            {
                return Err(io::Error::other("Invalid outcome records"));
            }
        }
        for (scope, change) in changes {
            let existing = records.get(scope);
            if existing.map(|r| r.episode) == change.expected
                || change.new_after_failed_load
                    && existing.is_some_and(|o| {
                        (o.observed_seconds, o.observed_nanos)
                            < (
                                change.outcome.observed_seconds,
                                change.outcome.observed_nanos,
                            )
                    })
            {
                if change.remove {
                    records.remove(scope);
                } else {
                    records.insert(scope.clone(), change.outcome.clone());
                }
            } else if let Some(existing) = existing
                && existing.episode == change.outcome.episode
            {
                // A concurrent dismissal of this instance wins over an unsaved capture.
                if change.outcome.dismissed && !existing.dismissed {
                    records.insert(scope.clone(), change.outcome.clone());
                }
            }
        }
        if records.len() > MAX_OUTCOMES {
            return Err(io::Error::other(
                "Outcome capacity reached; dismiss older outcomes",
            ));
        }
        let encoded = Saved {
            version: 1,
            records: records.values().cloned().collect(),
        }
        .encode_to_vec();
        if encoded.len() as u64 > MAX_BYTES {
            return Err(io::Error::other("Outcome journal exceeds its size limit"));
        }
        if encoded != bytes && !changes.is_empty() {
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            temp.write_all(&encoded)?;
            temp.as_file().sync_all()?;
            temp.persist(&self.path).map_err(|e| e.error)?;
        }
        Ok(records)
    }
}
fn valid(r: &Outcome) -> bool {
    r.source.len() == 2
        && r.source[0] == "coordinator"
        && !r.source[1].is_empty()
        && r.source.iter().all(|s| s.len() <= 128)
        && r.agent.len() == 10
        && r.agent[0] == "node"
        && r.agent[2] == "session"
        && r.agent[5] == "workspace"
        && r.agent[8] == "agent"
        && r.agent.iter().all(|s| s.len() <= 128)
        && ["done", "idle"].contains(&r.kind.as_str())
        && (0..1_000_000_000).contains(&r.observed_nanos)
        && r.names.len() == 4
        && r.names
            .iter()
            .all(|s| s.chars().count() <= 161 && !s.chars().any(char::is_control))
}
#[derive(Default)]
pub(crate) struct Tracker {
    pub store: Option<OutcomeStore>,
    records: BTreeMap<Scope, Outcome>,
    dirty: BTreeMap<Scope, Change>,
}
impl Tracker {
    pub fn new(store: Option<OutcomeStore>) -> Self {
        Self {
            store,
            ..Default::default()
        }
    }
    fn sync(&mut self) -> io::Result<()> {
        if let Some(store) = &self.store {
            self.records = store.transaction(&self.dirty)?;
        }
        self.dirty.clear();
        Ok(())
    }
    pub fn observe(
        &mut self,
        scene: &Scene,
        previous: Option<&Scene>,
        verified: bool,
        serial: &mut u64,
        done: &BTreeMap<Key, u64>,
        acknowledged: &BTreeMap<Key, u64>,
    ) -> (BTreeMap<Key, Outcome>, Option<String>) {
        let Some(coordinator) = &scene.coordinator else {
            return (BTreeMap::new(), None);
        };
        let source = &coordinator.key;
        let mut error = None;
        if verified && let Err(e) = self.sync() {
            error = Some(e.to_string());
        } else if !verified && previous.is_none() {
            self.records.retain(|(s, _), _| s != source);
        }
        fn collect<'a>(
            branch: &'a Branch,
            names: &mut Vec<String>,
            out: &mut BTreeMap<&'a Key, (&'a str, Vec<String>)>,
        ) {
            names.push(branch.label.clone());
            if branch.kind == "agent" {
                out.insert(&branch.key, (&branch.status, names.clone()));
            }
            for child in &branch.children {
                collect(child, names, out);
            }
            names.pop();
        }
        let mut current = BTreeMap::new();
        for n in &scene.nodes {
            collect(n, &mut Vec::new(), &mut current);
        }
        let mut prior = BTreeMap::new();
        if let Some(previous) = previous {
            for n in &previous.nodes {
                collect(n, &mut Vec::new(), &mut prior);
            }
        }
        let live_done: std::collections::BTreeSet<_> = current
            .iter()
            .filter(|(_, (s, _))| *s == "done")
            .map(|(k, _)| (*k).clone())
            .collect();
        for (agent, (state, names)) in current {
            let scope = (source.clone(), agent.clone());
            let old = self.records.get(&scope);
            let before = prior.get(agent).map(|(s, _)| *s);
            let new_done = state == "done"
                && (before.is_some_and(|s| s != "done")
                    || old.is_none()
                    || old.is_some_and(|o| o.kind != "done"));
            let new_idle = state == "idle" && before == Some("working");
            if !new_done && !new_idle {
                continue;
            }
            let expected = old.map(|o| o.episode);
            if !self.dirty.contains_key(&scope) && self.dirty.len() >= 2 * MAX_OUTCOMES {
                error =
                    Some("Outcome retry capacity reached; retained history was preserved".into());
                continue;
            }
            if old.is_none() && self.records.len() >= MAX_OUTCOMES {
                let recyclable = self
                    .records
                    .iter()
                    .find(|((s, a), o)| s == source && o.dismissed && !live_done.contains(a))
                    .map(|(s, o)| (s.clone(), o.clone()));
                if let Some((scope, outcome)) = recyclable {
                    self.records.remove(&scope);
                    if verified {
                        self.dirty.insert(
                            scope,
                            Change {
                                remove: true,
                                new_after_failed_load: false,
                                expected: Some(outcome.episode),
                                outcome,
                            },
                        );
                    }
                } else {
                    error = Some(
                        "Outcome capacity reached; retained unread history was preserved".into(),
                    );
                    continue;
                }
            }
            let episode = if state == "done" {
                done.get(agent).copied().unwrap_or_else(|| {
                    *serial = serial.wrapping_add(1);
                    *serial
                })
            } else {
                *serial = serial.wrapping_add(1);
                *serial
            };
            let outcome = Outcome {
                source: source.clone(),
                agent: agent.clone(),
                episode,
                kind: state.into(),
                names,
                dismissed: state == "done" && acknowledged.get(agent) == Some(&episode),
                observed_seconds: scene.observed_at.seconds,
                observed_nanos: scene.observed_at.nanos,
            };
            self.records.insert(scope.clone(), outcome.clone());
            if verified {
                let expected = self.dirty.get(&scope).map_or(expected, |c| c.expected);
                self.dirty.insert(
                    scope,
                    Change {
                        remove: false,
                        new_after_failed_load: expected.is_none()
                            && before.is_some()
                            && error.is_some(),
                        expected,
                        outcome,
                    },
                );
            }
        }
        if verified && let Err(e) = self.sync() {
            error = Some(e.to_string());
        }
        (self.publish(source), error)
    }
    pub fn publish(&self, source: &Key) -> BTreeMap<Key, Outcome> {
        self.records
            .iter()
            .filter(|((s, _), _)| s == source)
            .map(|(_, o)| (o.card_key(), o.clone()))
            .collect()
    }
    pub fn acknowledge(
        &mut self,
        source: &Key,
        agent: &Key,
        episode: u64,
        verified: bool,
    ) -> io::Result<bool> {
        let scope = (source.clone(), agent.clone());
        if verified {
            // A failed read must not discard an explicit click on a locally
            // observed outcome. The final transaction below reports durability.
            let _ = self.sync();
        }
        let Some(old) = self.records.get(&scope) else {
            return Ok(false);
        };
        if old.episode != episode {
            return Ok(false);
        }
        let mut next = old.clone();
        next.dismissed = true;
        self.records.insert(scope.clone(), next.clone());
        if verified {
            self.dirty.insert(
                scope,
                Change {
                    remove: false,
                    new_after_failed_load: self
                        .dirty
                        .get(&(source.clone(), agent.clone()))
                        .is_some_and(|c| c.new_after_failed_load),
                    expected: self
                        .dirty
                        .get(&(source.clone(), agent.clone()))
                        .map_or(Some(episode), |c| c.expected),
                    outcome: next,
                },
            );
            self.sync()?;
        }
        Ok(self
            .records
            .get(&(source.clone(), agent.clone()))
            .is_some_and(|o| o.episode == episode && o.dismissed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn outcome(agent: &str, episode: u64) -> Outcome {
        Outcome {
            source: vec!["coordinator".into(), "source".into()],
            agent: [
                "node",
                "n",
                "session",
                "default",
                "inc",
                "workspace",
                "w",
                "t",
                "agent",
                agent,
            ]
            .map(String::from)
            .to_vec(),
            episode,
            kind: "done".into(),
            names: vec![
                "Node".into(),
                "Session".into(),
                "Workspace".into(),
                "Agent".into(),
            ],
            dismissed: false,
            observed_seconds: episode as i64,
            observed_nanos: 0,
        }
    }
    fn change(o: Outcome, expected: Option<u64>) -> BTreeMap<Scope, Change> {
        BTreeMap::from([(
            (o.source.clone(), o.agent.clone()),
            Change {
                remove: false,
                new_after_failed_load: false,
                expected,
                outcome: o,
            },
        )])
    }
    #[test]
    fn concurrent_scopes_merge_and_stale_click_cannot_acknowledge_newer_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let store = OutcomeStore::at(dir.path().join("outcomes.bin"));
        let a = outcome("a", 1);
        let b = outcome("b", 2);
        store.transaction(&change(a.clone(), None)).unwrap();
        assert_eq!(store.transaction(&change(b, None)).unwrap().len(), 2);
        let scope = (a.source.clone(), a.agent.clone());
        let mut next = a.clone();
        next.episode = 3;
        store.transaction(&change(next.clone(), Some(1))).unwrap();
        let mut stale = a;
        stale.dismissed = true;
        assert_eq!(
            store.transaction(&change(stale, Some(1))).unwrap()[&scope],
            next
        );
        next.dismissed = true;
        store.transaction(&change(next.clone(), Some(3))).unwrap();
        let mut unsaved_capture = next.clone();
        unsaved_capture.dismissed = false;
        assert_eq!(
            store
                .transaction(&change(unsaved_capture, Some(1)))
                .unwrap()[&scope],
            next
        );
    }
    #[test]
    fn corrupt_empty_future_and_oversized_files_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outcomes.bin");
        let store = OutcomeStore::at(path.clone());
        for bytes in [
            vec![],
            vec![255],
            Saved {
                version: 2,
                records: vec![],
            }
            .encode_to_vec(),
            vec![0; MAX_BYTES as usize + 1],
        ] {
            fs::write(&path, &bytes).unwrap();
            assert!(store.transaction(&change(outcome("a", 1), None)).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
    #[test]
    fn capacity_never_discards_unread_outcomes_and_only_recycles_acknowledged_non_done() {
        let mut tracker = Tracker::default();
        for i in 0..MAX_OUTCOMES {
            let o = outcome(&format!("a{i}"), i as u64);
            tracker
                .records
                .insert((o.source.clone(), o.agent.clone()), o);
        }
        let scene = crate::projection::project(crate::pb::NodeList {
            nodes: vec![crate::pb::NodeView {
                instance_id: "n".into(),
                herdr: Some(crate::pb::HerdrState {
                    agents: vec![crate::pb::HerdrEntity {
                        id: "new".into(),
                        workspace_id: "w".into(),
                        agent_status: "done".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            }],
        })
        .unwrap();
        let mut scene = scene;
        scene.coordinator = Some(crate::projection::coordinator(Some(
            &crate::pb::ServerInfo {
                instance_id: "source".into(),
                ..Default::default()
            },
        )));
        let mut serial = 100_000;
        let (published, error) = tracker.observe(
            &scene,
            None,
            false,
            &mut serial,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        // Unverified baseline deliberately clears old namespaces. Use continuous
        // verified observations with an in-memory tracker for the capacity check.
        assert!(error.is_none());
        assert_eq!(published.len(), 1);
        tracker.records.clear();
        for i in 0..MAX_OUTCOMES {
            let o = outcome(&format!("a{i}"), i as u64);
            tracker
                .records
                .insert((o.source.clone(), o.agent.clone()), o);
        }
        let (_, error) = tracker.observe(
            &scene,
            None,
            true,
            &mut serial,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert!(error.is_some());
        assert_eq!(tracker.records.len(), MAX_OUTCOMES);
        tracker.records.values_mut().next().unwrap().dismissed = true;
        let (published, error) = tracker.observe(
            &scene,
            None,
            true,
            &mut serial,
            &BTreeMap::new(),
            &BTreeMap::new(),
        );
        assert!(error.is_none());
        assert_eq!(published.len(), MAX_OUTCOMES);
        assert!(published.values().any(|n| n.agent.last().unwrap() == "new"));
    }
}
