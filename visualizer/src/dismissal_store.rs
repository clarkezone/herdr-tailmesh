//! Local acknowledgements only. This is not a mesh state or task event journal.
use crate::projection::Key;
use prost::Message;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, Read, Write},
    path::PathBuf,
};

const VERSION: u32 = 1;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RECORDS: usize = 8000;

#[derive(Clone)]
pub(crate) struct Store {
    path: PathBuf,
}
// Private on-disk schema; deliberately separate from the observer protocol.
#[derive(Clone, PartialEq, Message)]
struct Saved {
    #[prost(uint32, tag = "1")]
    version: u32,
    #[prost(message, repeated, tag = "2")]
    records: Vec<Record>,
}
#[derive(Clone, PartialEq, Message)]
struct Record {
    #[prost(string, repeated, tag = "1")]
    source: Key,
    #[prost(string, repeated, tag = "2")]
    key: Key,
    #[prost(uint64, tag = "3")]
    episode: u64,
}
pub(crate) fn user_state_directory() -> io::Result<PathBuf> {
    fn env(name: &str) -> Option<PathBuf> {
        std::env::var_os(name)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    }
    #[cfg(target_os = "windows")]
    let root = env("LOCALAPPDATA");
    #[cfg(target_os = "macos")]
    let root = env("HOME").map(|p| p.join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let root = env("XDG_STATE_HOME")
        .filter(|p| p.is_absolute())
        .or_else(|| env("HOME").map(|p| p.join(".local/state")));
    let root = root.filter(|p| p.is_absolute()).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "User state directory is unavailable",
        )
    })?;
    Ok(root.join("herdr-mesh-visualizer"))
}

impl Store {
    pub(crate) fn at(path: PathBuf) -> Self {
        Self { path }
    }
    pub(crate) fn for_port(port: u16) -> io::Result<Self> {
        Ok(Self::at(
            user_state_directory()?.join(format!("dismissals-{port}.bin")),
        ))
    }
    fn read(&self) -> io::Result<Saved> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(Saved {
                    version: VERSION,
                    records: vec![],
                });
            }
            Err(e) => return Err(e),
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid("Acknowledgement file exceeds its size limit"));
        }
        let saved =
            Saved::decode(bytes.as_slice()).map_err(|_| invalid("Invalid acknowledgement file"))?;
        let mut unique = BTreeSet::new();
        if saved.version != VERSION
            || saved.records.len() > MAX_RECORDS
            || saved.records.iter().any(|r| {
                r.source.len() != 2
                    || r.source[0] != "coordinator"
                    || r.source[1].is_empty()
                    || r.source[1].len() > 128
                    || r.key.len() != 10
                    || r.key[0] != "node"
                    || r.key[2] != "session"
                    || r.key[5] != "workspace"
                    || r.key[8] != "agent"
                    || r.key.iter().any(|s| s.len() > 128)
                    || !unique.insert((&r.source, &r.key))
            })
        {
            return Err(invalid("Unsupported or invalid acknowledgement records"));
        }
        Ok(saved)
    }
    fn transaction<T>(&self, change: impl FnOnce(&mut Saved) -> io::Result<T>) -> io::Result<T> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| invalid("Missing state directory"))?;
        fs::create_dir_all(parent)?;
        // Lock a separate, never-replaced file. Bound waits rather than freezing the UI.
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.path.with_extension("lock"))?;
        lock.try_lock()?;
        let mut saved = self.read()?;
        let before = saved.clone();
        let result = change(&mut saved)?;
        if saved != before {
            let bytes = saved.encode_to_vec();
            if saved.records.len() > MAX_RECORDS || bytes.len() as u64 > MAX_BYTES {
                return Err(invalid("Acknowledgement capacity reached"));
            }
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            temp.write_all(&bytes)?;
            temp.as_file().sync_all()?;
            temp.persist(&self.path).map_err(|e| e.error)?;
        }
        Ok(result)
    }
    pub(crate) fn restore(
        &self,
        source: &Key,
        done: &BTreeMap<Key, u64>,
    ) -> io::Result<BTreeMap<Key, u64>> {
        self.transaction(|saved| {
            saved
                .records
                .retain(|r| &r.source != source || done.contains_key(&r.key));
            Ok(saved
                .records
                .iter()
                .filter(|r| &r.source == source)
                .map(|r| (r.key.clone(), r.episode))
                .collect())
        })
    }
    pub(crate) fn acknowledge(&self, source: &Key, key: &Key, episode: u64) -> io::Result<()> {
        self.transaction(|saved| {
            saved
                .records
                .retain(|r| &r.source != source || &r.key != key);
            saved.records.push(Record {
                source: source.clone(),
                key: key.clone(),
                episode,
            });
            Ok(())
        })
    }
    pub(crate) fn retire(&self, source: &Key, retired: &BTreeMap<Key, u64>) -> io::Result<()> {
        self.transaction(|saved| {
            // Another viewer may have acknowledged a newer instance in the meantime.
            saved
                .records
                .retain(|r| &r.source != source || retired.get(&r.key) != Some(&r.episode));
            Ok(())
        })
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(agent: &str) -> Key {
        [
            "node",
            "n",
            "session",
            "s",
            "inc",
            "workspace",
            "w",
            "tab",
            "agent",
            agent,
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }
    fn source(id: &str) -> Key {
        vec!["coordinator".into(), id.into()]
    }
    #[test]
    fn independent_writers_merge_and_old_retirement_cannot_erase_a_new_acknowledgement() {
        let dir = tempfile::tempdir().unwrap();
        let a = Store::at(dir.path().join("state.bin"));
        let b = a.clone();
        a.acknowledge(&source("one"), &key("a"), 1).unwrap();
        b.acknowledge(&source("one"), &key("b"), 2).unwrap();
        b.acknowledge(&source("one"), &key("a"), 3).unwrap();
        a.retire(&source("one"), &BTreeMap::from([(key("a"), 1)]))
            .unwrap();
        let done = BTreeMap::from([(key("a"), 0), (key("b"), 0)]);
        assert_eq!(
            a.restore(&source("one"), &done).unwrap(),
            BTreeMap::from([(key("a"), 3), (key("b"), 2)])
        );
        assert!(a.restore(&source("two"), &done).unwrap().is_empty());
        assert_eq!(a.restore(&source("one"), &done).unwrap().len(), 2);
    }
    #[test]
    fn corrupt_oversized_or_future_state_is_not_overwritten_and_lock_contention_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::at(dir.path().join("state.bin"));
        for bytes in [
            vec![0xff],
            Saved {
                version: 2,
                records: vec![],
            }
            .encode_to_vec(),
            vec![0; MAX_BYTES as usize + 1],
        ] {
            fs::write(&store.path, &bytes).unwrap();
            assert!(store.acknowledge(&source("one"), &key("a"), 1).is_err());
            assert_eq!(fs::read(&store.path).unwrap(), bytes);
        }
        fs::remove_file(&store.path).unwrap();
        let lock = File::options()
            .read(true)
            .write(true)
            .open(store.path.with_extension("lock"))
            .unwrap();
        lock.try_lock().unwrap();
        assert_eq!(
            store
                .acknowledge(&source("one"), &key("a"), 1)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(lock);
        store.acknowledge(&source("one"), &key("a"), 1).unwrap();
    }
    #[test]
    fn record_capacity_and_duplicate_keys_fail_without_losing_previous_acknowledgements() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::at(dir.path().join("state.bin"));
        let saved = Saved {
            version: VERSION,
            records: (0..MAX_RECORDS)
                .map(|i| Record {
                    source: source("one"),
                    key: key(&i.to_string()),
                    episode: i as u64,
                })
                .collect(),
        };
        let bytes = saved.encode_to_vec();
        fs::write(&store.path, &bytes).unwrap();
        assert!(
            store
                .acknowledge(&source("one"), &key("new"), 9000)
                .is_err()
        );
        assert_eq!(fs::read(&store.path).unwrap(), bytes);
        let duplicate = Saved {
            version: VERSION,
            records: vec![saved.records[0].clone(), saved.records[0].clone()],
        }
        .encode_to_vec();
        fs::write(&store.path, &duplicate).unwrap();
        assert!(store.acknowledge(&source("one"), &key("a"), 1).is_err());
        assert_eq!(fs::read(&store.path).unwrap(), duplicate);
    }
}
