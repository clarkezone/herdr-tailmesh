//! Change-based completion diagnostics. Never logs snapshots, paths or credentials.
//! File writes run on a bounded background queue, independent of RUST_LOG.
use crate::dismissal_store::user_state_directory;
pub use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const FILE_BYTES: u64 = 2 * 1024 * 1024;
const FILES: usize = 3;
const SLOTS: usize = 8;
const QUEUE: usize = 1024;
const RECORD_BYTES: usize = 32 * 1024;
static RUNS: AtomicU64 = AtomicU64::new(0);

enum Message {
    Record(Vec<u8>),
    Flush(mpsc::Sender<()>),
}
struct Producer {
    sender: Option<SyncSender<Message>>,
    sequence: u64,
    pending_loss: u64,
}
pub struct Diagnostics {
    producer: Mutex<Producer>,
    worker: Option<JoinHandle<()>>,
    path: PathBuf,
    port: u16,
    run: String,
    started: Instant,
    losses: Arc<AtomicU64>,
    failed: AtomicBool,
    renderer: AtomicU64,
    separated_tail: bool,
}
struct Sink {
    // Keep the slot locked across rotations and until the writer stops.
    _lock: File,
    path: PathBuf,
    file: Option<File>,
    bytes: u64,
    limit: u64,
    separated_tail: bool,
}
fn millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
impl Sink {
    fn open(directory: &Path, port: u16, limit: u64) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        for slot in 0..SLOTS {
            let path = directory.join(format!("completion-{port}-{slot}.jsonl"));
            let lock = File::options()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path.with_extension("lock"))?;
            if lock.try_lock().is_err() {
                continue;
            }
            let mut file = File::options()
                .read(true)
                .append(true)
                .create(true)
                .open(&path)?;
            let mut bytes = file.metadata()?.len();
            let mut separated_tail = false;
            if bytes > 0 {
                file.seek(SeekFrom::End(-1))?;
                let mut last = [0];
                file.read_exact(&mut last)?;
                if last[0] != b'\n' {
                    // Preserve crash evidence, but don't join the next launch onto a partial record.
                    file.write_all(b"\n")?;
                    bytes += 1;
                    separated_tail = true;
                }
            }
            return Ok(Self {
                _lock: lock,
                path,
                file: Some(file),
                bytes,
                limit,
                separated_tail,
            });
        }
        Err(io::Error::other(
            "All eight completion diagnostic writer slots are in use",
        ))
    }
    fn rotated(&self, index: usize) -> PathBuf {
        self.path.with_extension(format!("{index}.jsonl"))
    }
    fn rotate(&mut self) -> io::Result<()> {
        // Close before rename, including on Windows.
        self.file.take();
        for index in (1..FILES).rev() {
            let to = self.rotated(index);
            match fs::remove_file(&to) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            let from = if index == 1 {
                self.path.clone()
            } else {
                self.rotated(index - 1)
            };
            match fs::rename(from, to) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        self.file = Some(File::options().append(true).create(true).open(&self.path)?);
        self.bytes = 0;
        Ok(())
    }
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.bytes > 0 && self.bytes.saturating_add(bytes.len() as u64) > self.limit {
            self.rotate()?;
        }
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("Diagnostic writer closed"))?
            .write_all(bytes)?;
        self.bytes += bytes.len() as u64;
        Ok(())
    }
}
impl Diagnostics {
    pub fn start(port: u16, launcher: &str, mode: &str, renderer: &str) -> io::Result<Arc<Self>> {
        let log = Self::at(&user_state_directory()?.join("logs"), port)?;
        log.event(
            "launch",
            json!({
                "version": env!("CARGO_PKG_VERSION"),
                "git_commit": env!("HERDR_VISUALIZER_GIT_COMMIT"),
                "git_dirty": env!("HERDR_VISUALIZER_GIT_DIRTY"),
                "launcher": launcher, "mode": mode, "renderer": renderer,
                "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "file_bytes": FILE_BYTES, "files_per_slot": FILES, "writer_slots": SLOTS,
            "previous_tail_separated":log.separated_tail,
            }),
        );
        Ok(log)
    }
    /// Separate files/locks make concurrent viewers and tests independent.
    pub fn at(directory: &Path, port: u16) -> io::Result<Arc<Self>> {
        Self::with_limit(directory, port, FILE_BYTES)
    }
    fn with_limit(directory: &Path, port: u16, limit: u64) -> io::Result<Arc<Self>> {
        let mut sink = Sink::open(directory, port, limit)?;
        let path = sink.path.clone();
        let separated_tail = sink.separated_tail;
        let (sender, receiver) = mpsc::sync_channel(QUEUE);
        let losses = Arc::new(AtomicU64::new(0));
        let writer_losses = losses.clone();
        let run = format!(
            "{}-{}-{}-{}",
            std::process::id(),
            millis(),
            RUNS.fetch_add(1, Ordering::Relaxed),
            path.file_name().unwrap().to_string_lossy()
        );
        let writer_run = run.clone();
        let worker = thread::Builder::new().name("completion-diagnostics".into()).spawn(move || {
            let result = (|| -> io::Result<()> {
                while let Ok(message) = receiver.recv() {
                    match message {
                        Message::Record(bytes) => sink.write(&bytes)?,
                        Message::Flush(done) => { let _ = done.send(()); },
                    }
                }
                let lost = writer_losses.load(Ordering::Relaxed);
                if lost > 0 {
                    let mut bytes = serde_json::to_vec(&json!({"event":"diagnostic_loss", "run":writer_run, "dropped_total":lost}))?;
                    bytes.push(b'\n');
                    sink.write(&bytes)?;
                }
                Ok(())
            })();
            if let Err(error) = result { eprintln!("Completion diagnostic writer failed: {error}"); }
        })?;
        Ok(Arc::new(Self {
            producer: Mutex::new(Producer {
                sender: Some(sender),
                sequence: 0,
                pending_loss: 0,
            }),
            worker: Some(worker),
            path,
            port,
            run,
            started: Instant::now(),
            losses,
            failed: AtomicBool::new(false),
            renderer: AtomicU64::new(0),
            separated_tail,
        }))
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn new_renderer(&self) -> u64 {
        self.renderer.fetch_add(1, Ordering::Relaxed) + 1
    }
    pub fn event(&self, event: &str, mut data: Value) {
        // Launch counters are random u64 values. Keep opaque IDs exact in JS/JSON tools too.
        for field in [
            "episode",
            "previous_episode",
            "current_episode",
            "activity_episode",
            "idle_stop_episode",
        ] {
            if let Some(value) = data.get_mut(field)
                && let Some(id) = value.as_u64()
            {
                *value = Value::String(id.to_string());
            }
        }
        let mut producer = self.producer.lock().unwrap();
        producer.sequence += 1;
        let mut bytes = serde_json::to_vec(&json!({
            "event":event, "data":data, "seq":producer.sequence,
            "run":self.run, "port":self.port, "pid":std::process::id(),
            "unix_ms":millis(), "elapsed_ms":self.started.elapsed().as_millis(),
            "dropped_before":producer.pending_loss,
            "build_commit":env!("HERDR_VISUALIZER_GIT_COMMIT"),
            "build_dirty":env!("HERDR_VISUALIZER_GIT_DIRTY"),
        }))
        .expect("Diagnostic JSON values are serializable");
        bytes.push(b'\n');
        if bytes.len() > RECORD_BYTES {
            producer.pending_loss += 1;
            self.losses.fetch_add(1, Ordering::Relaxed);
            return;
        }
        match producer
            .sender
            .as_ref()
            .unwrap()
            .try_send(Message::Record(bytes))
        {
            Ok(()) => producer.pending_loss = 0,
            Err(TrySendError::Full(_)) => {
                producer.pending_loss += 1;
                self.losses.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => {
                if !self.failed.swap(true, Ordering::Relaxed) {
                    eprintln!("Completion diagnostics unavailable: writer stopped");
                }
            }
        }
    }
    /// Used only for shutdown/tests, never by a rendering frame.
    pub fn flush(&self) {
        let (done, completed) = mpsc::channel();
        if self
            .producer
            .lock()
            .unwrap()
            .sender
            .as_ref()
            .unwrap()
            .send(Message::Flush(done))
            .is_ok()
        {
            let _ = completed.recv_timeout(Duration::from_secs(5));
        }
    }
}
impl Drop for Diagnostics {
    fn drop(&mut self) {
        self.producer.get_mut().unwrap().sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escaped_records_rotation_restart_and_concurrent_writer_slots() {
        let dir = tempfile::tempdir().unwrap();
        let a = Diagnostics::with_limit(dir.path(), 8790, 600).unwrap();
        let b = Diagnostics::at(dir.path(), 8790).unwrap();
        assert_ne!(a.path(), b.path());
        for i in 0..8 {
            a.event("state", json!({"i":i, "key":["agent", "quoted\"\n雪"]}));
        }
        a.flush();
        let path = a.path().to_owned();
        let mut found = 0;
        for path in [
            path.clone(),
            path.with_extension("1.jsonl"),
            path.with_extension("2.jsonl"),
        ] {
            let contents = fs::read_to_string(path).unwrap();
            for line in contents.lines() {
                let value: Value = serde_json::from_str(line).unwrap();
                assert_eq!(value["data"]["key"][1], "quoted\"\n雪");
                found += 1;
            }
        }
        assert!(found > 0 && found < 8);
        drop(a);
        let restarted = Diagnostics::at(dir.path(), 8790).unwrap();
        assert_eq!(restarted.path(), path);
        restarted.event("relaunch", json!({}));
        restarted.flush();
        assert!(fs::read_to_string(path).unwrap().contains("relaunch"));
    }
    #[test]
    fn oversized_records_are_disclosed_without_changing_observation() {
        let dir = tempfile::tempdir().unwrap();
        let log = Diagnostics::at(dir.path(), 1).unwrap();
        log.event("too_big", json!({"text":"x".repeat(RECORD_BYTES)}));
        log.event("next", json!({}));
        log.flush();
        let path = log.path().to_owned();
        drop(log);
        let records: Vec<Value> = fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(records[0]["dropped_before"], 1);
        assert_eq!(records[1]["dropped_total"], 1);
    }
    #[test]
    fn lock_capacity_and_unwritable_directory_fail_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let logs: Vec<_> = (0..SLOTS)
            .map(|_| Diagnostics::at(dir.path(), 1).unwrap())
            .collect();
        assert!(Diagnostics::at(dir.path(), 1).is_err());
        let file = dir.path().join("file");
        fs::write(&file, b"keep").unwrap();
        assert!(Diagnostics::at(&file, 1).is_err());
        assert_eq!(fs::read(file).unwrap(), b"keep");
        drop(logs);
    }
    #[test]
    fn full_queue_does_not_block_and_the_next_record_discloses_loss() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = Diagnostics::at(dir.path(), 1).unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        let mutable = Arc::get_mut(&mut log).unwrap();
        mutable.producer.get_mut().unwrap().sender.replace(sender);
        mutable.worker.take().unwrap().join().unwrap();
        log.event("first", json!({}));
        log.event("dropped", json!({}));
        assert!(matches!(receiver.try_recv().unwrap(), Message::Record(_)));
        log.event("next", json!({}));
        let Message::Record(bytes) = receiver.try_recv().unwrap() else {
            panic!("record");
        };
        let record: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(record["dropped_before"], 1);
        assert_eq!(record["seq"], 3);
        drop(receiver);
        log.event("writer_stopped", json!({}));
        assert!(log.failed.load(Ordering::Relaxed));
    }
    #[test]
    fn a_rotation_write_failure_stops_only_the_diagnostic_writer() {
        let dir = tempfile::tempdir().unwrap();
        let log = Diagnostics::with_limit(dir.path(), 1, 1).unwrap();
        log.event("first", json!({}));
        log.flush();
        // A directory in place of the next rotation forces a real filesystem error.
        fs::create_dir(log.path().with_extension("2.jsonl")).unwrap();
        log.event("rotation_failure", json!({}));
        log.flush();
        log.event("observation_continues", json!({}));
        assert!(log.failed.load(Ordering::Relaxed));
        let first = fs::read_to_string(log.path()).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(first.trim()).unwrap()["event"],
            "first"
        );
    }
    #[test]
    fn partial_crash_tails_are_separated_and_episode_ids_keep_full_precision() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("completion-1-0.jsonl");
        fs::write(&path, b"{\"event\":\"old\"}\n{\"event\":\"unfinished\"").unwrap();
        let log = Diagnostics::at(dir.path(), 1).unwrap();
        assert!(log.separated_tail);
        log.event(
            "new",
            json!({"episode":u64::MAX, "previous_episode":null, "current_episode":u64::MAX-1}),
        );
        log.flush();
        let contents = fs::read_to_string(path).unwrap();
        let lines: Vec<_> = contents.lines().collect();
        assert_eq!(lines[1], "{\"event\":\"unfinished\"");
        let new: Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(new["event"], "new");
        assert_eq!(new["data"]["episode"], u64::MAX.to_string());
        assert_eq!(new["data"]["current_episode"], (u64::MAX - 1).to_string());
        assert!(new["data"]["previous_episode"].is_null());
    }
}
