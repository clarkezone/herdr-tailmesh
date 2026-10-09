use crate::{
    diagnostics::{Diagnostics, Value, json},
    dismissal_store::Store,
    pb::local_observer_client::LocalObserverClient,
    projection::{Branch, Key, Scene, coordinator, project},
};
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tonic::{Code, Request, transport::Endpoint};

pub const MAX_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const FIRST_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct View {
    pub status: String,
    pub live: bool,
    pub daemon: String,
    pub scene: Option<Arc<Scene>>,
    pub received: Option<Instant>,
    pub epoch: u64,
    pub revision: u64,
    /// IDs of currently observed completion episodes, updated before UI coalescing.
    pub completion_episodes: Arc<BTreeMap<Key, u64>>,
    /// Directly observed Working -> Idle stops, retained only while still Idle.
    pub idle_stop_episodes: Arc<BTreeMap<Key, u64>>,
    /// Session-only acknowledgement of observed Idle stops; never stored as Done.
    pub acknowledged_idle_stops: Arc<BTreeMap<Key, u64>>,
    /// Local acknowledgements restored before the first scene is published.
    pub acknowledged_completions: Arc<BTreeMap<Key, u64>>,
    pub acknowledgement_revision: u64,
    pub persistence_error: Option<String>,
    pub diagnostics: Option<Arc<Diagnostics>>,
}
impl Default for View {
    fn default() -> Self {
        Self {
            status: "Connecting to local daemon…".into(),
            live: false,
            daemon: String::new(),
            scene: None,
            received: None,
            epoch: 0,
            revision: 0,
            completion_episodes: Default::default(),
            idle_stop_episodes: Default::default(),
            acknowledged_idle_stops: Default::default(),
            acknowledged_completions: Default::default(),
            acknowledgement_revision: 0,
            persistence_error: None,
            diagnostics: None,
        }
    }
}
impl View {
    pub fn stop_episode(&self, key: &Key) -> Option<u64> {
        self.completion_episodes
            .get(key)
            .or_else(|| self.idle_stop_episodes.get(key))
            .copied()
    }
    pub fn stop_episodes(&self) -> impl Iterator<Item = (&Key, &u64)> {
        self.completion_episodes
            .iter()
            .chain(self.idle_stop_episodes.iter())
    }
    pub fn stop_acknowledged(&self, key: &Key, episode: u64) -> bool {
        self.acknowledged_completions.get(key) == Some(&episode)
            && self.completion_episodes.get(key) == Some(&episode)
            || self.acknowledged_idle_stops.get(key) == Some(&episode)
                && self.idle_stop_episodes.get(key) == Some(&episode)
    }
    pub fn diagnostic(&self, event: &str, mut data: Value) {
        if let Some(log) = &self.diagnostics {
            data["epoch"] = json!(self.epoch);
            data["revision"] = json!(self.revision);
            data["source"] = json!(
                self.scene
                    .as_ref()
                    .and_then(|s| s.coordinator.as_ref())
                    .map(|c| &c.key)
            );
            log.event(event, data);
        }
    }
    fn trace_receipt(&self, previous: &View, agents: &mut BTreeMap<Key, String>) {
        fn collect(branch: &Branch, agents: &mut BTreeMap<Key, (String, String)>) {
            if branch.kind == "agent" {
                agents.insert(
                    branch.key.clone(),
                    (branch.status.clone(), branch.label.clone()),
                );
            }
            for child in &branch.children {
                collect(child, agents);
            }
        }
        let old_source = previous
            .scene
            .as_ref()
            .and_then(|s| s.coordinator.as_ref())
            .map(|c| &c.key);
        let source = self
            .scene
            .as_ref()
            .and_then(|s| s.coordinator.as_ref())
            .map(|c| &c.key);
        if old_source != source {
            self.diagnostic("source_changed", json!({"previous_source":old_source}));
            agents.clear();
        }
        let mut current = BTreeMap::new();
        if let Some(scene) = &self.scene {
            for node in &scene.nodes {
                collect(node, &mut current);
            }
            self.diagnostic("snapshot_accepted", json!({
                "observed_seconds":scene.observed_at.seconds, "observed_nanos":scene.observed_at.nanos,
                "nodes":scene.nodes.len(), "agents":current.len(),
                "completed":self.completion_episodes.len(), "idle_stops":self.idle_stop_episodes.len(), "working":scene.working, "blocked":scene.blocked,
            }));
        }
        for (key, (state, label)) in &current {
            if agents.get(key) != Some(state) {
                self.diagnostic(
                    "agent_state",
                    json!({
                        "key":key, "label":label, "previous_state":agents.get(key), "state":state,
                        "episode":self.completion_episodes.get(key),
                        "idle_stop_episode":self.idle_stop_episodes.get(key),
                    }),
                );
            }
        }
        for (key, state) in agents.iter().filter(|(key, _)| !current.contains_key(*key)) {
            self.diagnostic("agent_removed", json!({"key":key, "previous_state":state}));
        }
        for (key, episode) in previous.completion_episodes.iter() {
            if old_source != source || self.completion_episodes.get(key) != Some(episode) {
                self.diagnostic(
                    "completion_retired",
                    json!({"key":key, "episode":episode, "previous_source":old_source}),
                );
            }
        }
        for (key, episode) in self.completion_episodes.iter() {
            if old_source != source || previous.completion_episodes.get(key) != Some(episode) {
                self.diagnostic("completion_created", json!({"key":key, "episode":episode}));
            }
        }
        for (key, episode) in previous.idle_stop_episodes.iter() {
            if old_source != source || self.idle_stop_episodes.get(key) != Some(episode) {
                self.diagnostic("idle_stop_retired", json!({"key":key, "episode":episode}));
            }
        }
        for (key, episode) in self.idle_stop_episodes.iter() {
            if old_source != source || previous.idle_stop_episodes.get(key) != Some(episode) {
                self.diagnostic("idle_stop_created", json!({"key":key, "episode":episode, "previous_state":"working", "state":"idle"}));
            }
        }
        *agents = current.into_iter().map(|(k, (s, _))| (k, s)).collect();
    }
}
pub struct Shared {
    received: Mutex<Received>,
    wake_pending: AtomicBool,
    wake: Box<dyn Fn() + Send + Sync>,
    store: Option<Store>,
}
#[derive(Default)]
struct Received {
    view: View,
    completion_serial: u64,
    restored_source: Option<Key>,
    restore_candidates: BTreeMap<Key, u64>,
    unsaved_acknowledgements: BTreeMap<Key, u64>,
    diagnostic_agents: BTreeMap<Key, String>,
}
fn verified_source(view: &View) -> Option<Key> {
    view.scene
        .as_ref()?
        .coordinator
        .as_ref()
        .filter(|c| c.status == "verified upstream identity")
        .map(|c| c.key.clone())
}
fn observed_agent_statuses(view: &View) -> BTreeMap<&Key, &str> {
    if view.diagnostics.is_some()
        && let Some(scene) = &view.scene
    {
        agent_statuses(scene)
    } else {
        BTreeMap::new()
    }
}
fn storage_error(view: &mut View, error: impl std::fmt::Display) {
    let message = format!("Completion dismissals could not be saved/restored: {error}");
    if view.persistence_error.as_ref() != Some(&message) {
        log::warn!("{message}");
        view.diagnostic("ack_storage_error", json!({"error":message}));
    }
    view.persistence_error = Some(message);
}
fn completions(
    branch: &Branch,
    previous: Option<&BTreeMap<Key, u64>>,
    serial: &mut u64,
    current: &mut BTreeMap<Key, u64>,
) {
    if branch.kind == "agent" && branch.status == "done" {
        let episode = previous
            .and_then(|p| p.get(&branch.key))
            .copied()
            .unwrap_or_else(|| {
                *serial = serial.wrapping_add(1);
                *serial
            });
        current.insert(branch.key.clone(), episode);
    }
    for child in &branch.children {
        completions(child, previous, serial, current);
    }
}
fn agent_statuses(scene: &Scene) -> BTreeMap<&Key, &str> {
    fn collect<'a>(branch: &'a Branch, states: &mut BTreeMap<&'a Key, &'a str>) {
        if branch.kind == "agent" {
            states.insert(&branch.key, &branch.status);
        }
        for child in &branch.children {
            collect(child, states);
        }
    }
    let mut result = BTreeMap::new();
    for node in &scene.nodes {
        collect(node, &mut result);
    }
    result
}
fn idle_stops(
    scene: &Scene,
    previous: &BTreeMap<Key, u64>,
    previous_states: &BTreeMap<&Key, &str>,
    serial: &mut u64,
) -> BTreeMap<Key, u64> {
    agent_statuses(scene)
        .into_iter()
        .filter_map(|(key, state)| {
            if state != "idle" {
                return None;
            }
            let episode = if let Some(episode) = previous.get(key) {
                *episode
            } else if previous_states.get(key) == Some(&"working") {
                *serial = serial.wrapping_add(1);
                *serial
            } else {
                return None;
            };
            Some((key.clone(), episode))
        })
        .collect()
}
impl Shared {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        Self::with_store(wake, None, 0)
    }
    /// Native launchers share per-user, per-loopback-port acknowledgement storage.
    pub fn persistent(port: u16, wake: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        let mut random = [0; 8];
        let store = Store::for_port(port).and_then(|store| {
            getrandom::fill(&mut random).map_err(std::io::Error::other)?;
            Ok(store)
        });
        match store {
            Ok(store) => Self::with_store(wake, Some(store), u64::from_le_bytes(random)),
            Err(error) => {
                let shared = Self::new(wake);
                storage_error(&mut shared.received.lock().unwrap().view, error);
                shared
            }
        }
    }
    fn with_store(
        wake: impl Fn() + Send + Sync + 'static,
        store: Option<Store>,
        serial: u64,
    ) -> Arc<Self> {
        Arc::new(Self {
            received: Mutex::new(Received {
                completion_serial: serial,
                ..Default::default()
            }),
            wake_pending: AtomicBool::new(false),
            wake: Box::new(wake),
            store,
        })
    }
    pub fn read(&self) -> View {
        self.wake_pending.store(false, Ordering::Release);
        self.received.lock().unwrap().view.clone()
    }
    pub fn attach_diagnostics(&self, diagnostics: Arc<Diagnostics>) {
        let mut received = self.received.lock().unwrap();
        received.view.diagnostics = Some(diagnostics);
        received.view.diagnostic(
            "ack_storage_config",
            json!({
                "enabled":self.store.is_some(), "error":received.view.persistence_error,
            }),
        );
    }
    fn diagnostic(&self, event: &str, data: Value) {
        // Instrumentation must not consume the UI wake_pending flag via read().
        self.received.lock().unwrap().view.diagnostic(event, data);
    }
    fn update(&self, f: impl FnOnce(&mut View)) {
        {
            let mut received = self.received.lock().unwrap();
            let Received {
                view,
                completion_serial,
                restored_source,
                restore_candidates,
                unsaved_acknowledgements,
                diagnostic_agents,
            } = &mut *received;
            let diagnostic_previous = view.diagnostics.as_ref().map(|_| view.clone());
            let revision = view.revision;
            let previous_scene = view.scene.clone();
            let previous_live = view.live;
            let previous_verified_source = verified_source(view);
            let source = view
                .scene
                .as_ref()
                .and_then(|s| s.coordinator.as_ref())
                .map(|c| c.key.clone());
            f(view);
            if view.revision != revision {
                let mut current = BTreeMap::new();
                let mut idle_current = BTreeMap::new();
                if let Some(scene) = &view.scene {
                    let same_source = source == scene.coordinator.as_ref().map(|c| c.key.clone());
                    let previous = same_source.then_some(view.completion_episodes.as_ref());
                    for node in &scene.nodes {
                        completions(node, previous, completion_serial, &mut current);
                    }
                    let continuous =
                        same_source && (verified_source(view).is_some() || previous_live);
                    if continuous && let Some(previous_scene) = &previous_scene {
                        idle_current = idle_stops(
                            scene,
                            &view.idle_stop_episodes,
                            &agent_statuses(previous_scene),
                            completion_serial,
                        );
                    }
                }
                let source = verified_source(view);
                if (source.is_none() || self.store.is_none())
                    && (revision == 0 || source != previous_verified_source)
                {
                    view.diagnostic("ack_restore_unavailable", json!({
                        "reason":if self.store.is_none() { "storage_unavailable" } else { "no_verified_source" },
                    }));
                }
                if source != previous_verified_source {
                    // Retrying a failed load must not restore a completion that
                    // left Done while storage was unavailable.
                    *restore_candidates = current.clone();
                    unsaved_acknowledgements.clear();
                    view.acknowledged_completions = Default::default();
                    view.acknowledgement_revision = view.acknowledgement_revision.wrapping_add(1);
                } else {
                    restore_candidates.retain(|key, id| current.get(key) == Some(id));
                }
                if source != *restored_source {
                    if let (Some(store), Some(source)) = (&self.store, &source) {
                        view.diagnostic(
                            "ack_restore_attempt",
                            json!({"candidates":restore_candidates.len()}),
                        );
                        match store.restore(source, restore_candidates) {
                            Ok(restoration) => {
                                view.diagnostic("ack_restore_result", json!({
                                    "file_present":restoration.file_present,
                                    "matched":restoration.acknowledged.len(),
                                    "excluded":restoration.excluded.len(),
                                    "other_source_records":restoration.other_sources.values().sum::<usize>(),
                                }));
                                for (saved_source, records) in &restoration.other_sources {
                                    view.diagnostic(
                                        "ack_restore_source_mismatch",
                                        json!({
                                            "saved_source":saved_source, "records":records,
                                        }),
                                    );
                                }
                                let states = observed_agent_statuses(view);
                                for (key, id) in &restoration.excluded {
                                    let state = states.get(key).copied();
                                    view.diagnostic(
                                        "ack_restore_excluded",
                                        json!({
                                            "key":key, "episode":id, "state":state,
                                            "current_episode":current.get(key),
                                            "reason":match state {
                                                None => "agent_missing",
                                                Some("done") => "completion_changed_during_restore",
                                                Some(_) => "agent_not_done",
                                            },
                                            "retired_from_disk":true,
                                        }),
                                    );
                                }
                                let mut acknowledged = restoration.acknowledged;
                                for (key, id) in &acknowledged {
                                    view.diagnostic(
                                        "ack_restored",
                                        json!({"key":key, "episode":id}),
                                    );
                                }
                                // Preserve explicit local input during a temporary load failure.
                                for (key, id) in view.acknowledged_completions.iter() {
                                    if current.get(key) == Some(id) {
                                        acknowledged.insert(key.clone(), *id);
                                    }
                                }
                                // Restore only actual Done membership, before the first UI frame.
                                for (key, id) in &acknowledged {
                                    current.insert(key.clone(), *id);
                                }
                                view.acknowledged_completions = Arc::new(acknowledged);
                                view.acknowledgement_revision =
                                    view.acknowledgement_revision.wrapping_add(1);
                                view.persistence_error = None;
                                *restored_source = Some(source.clone());
                                restore_candidates.clear();
                            }
                            Err(error) => {
                                view.diagnostic("ack_restore_failed", json!({"error_kind":format!("{:?}",error.kind()), "error":error.to_string()}));
                                storage_error(view, error);
                            }
                        }
                    } else {
                        *restored_source = source.clone();
                        restore_candidates.clear();
                    }
                } else {
                    let retired: BTreeMap<_, _> = view
                        .acknowledged_completions
                        .iter()
                        .filter(|(key, id)| current.get(*key) != Some(*id))
                        .map(|(key, id)| (key.clone(), *id))
                        .collect();
                    if !retired.is_empty() {
                        let mut saved = true;
                        if let (Some(store), Some(source)) = (&self.store, source.as_ref()) {
                            match store.retire(source, &retired) {
                                Ok(()) => view.persistence_error = None,
                                Err(error) => {
                                    saved = false;
                                    storage_error(view, error);
                                }
                            }
                        }
                        // Mismatching IDs never suppress cards. Keep failed retirements
                        // pending here so later accepted snapshots retry the disk update.
                        if saved {
                            let states = observed_agent_statuses(view);
                            for (key, id) in &retired {
                                view.diagnostic("ack_retired", json!({"key":key, "episode":id, "state":states.get(key), "current_episode":current.get(key)}));
                            }
                            Arc::make_mut(&mut view.acknowledged_completions)
                                .retain(|key, _| !retired.contains_key(key));
                            view.acknowledgement_revision =
                                view.acknowledgement_revision.wrapping_add(1);
                        }
                    }
                }
                unsaved_acknowledgements.retain(|key, id| current.get(key) == Some(id));
                if let (Some(store), Some(source)) = (&self.store, source.as_ref()) {
                    unsaved_acknowledgements.retain(|key, id| {
                        match store.acknowledge(source, key, *id) {
                            Ok(()) => {
                                view.persistence_error = None;
                                view.diagnostic(
                                    "ack_saved_retry",
                                    json!({"key":key, "episode":id}),
                                );
                                false
                            }
                            Err(error) => {
                                storage_error(view, error);
                                true
                            }
                        }
                    });
                }
                // Keep only current completions; Working/removal retires the prior episode.
                view.completion_episodes = Arc::new(current);
                if view
                    .acknowledged_idle_stops
                    .iter()
                    .any(|(key, id)| idle_current.get(key) != Some(id))
                {
                    Arc::make_mut(&mut view.acknowledged_idle_stops)
                        .retain(|key, id| idle_current.get(key) == Some(id));
                    view.acknowledgement_revision = view.acknowledgement_revision.wrapping_add(1);
                }
                view.idle_stop_episodes = Arc::new(idle_current);
                if let Some(previous) = &diagnostic_previous {
                    view.trace_receipt(previous, diagnostic_agents);
                }
            }
        }
        if !self.wake_pending.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
    }
    /// Revalidate the clicked frame against the latest accepted completion.
    pub fn acknowledge(&self, key: &Key, episode: u64) -> bool {
        let mut received = self.received.lock().unwrap();
        let view = &mut received.view;
        view.diagnostic("ack_requested", json!({"key":key, "episode":episode}));
        if view.idle_stop_episodes.get(key) == Some(&episode) {
            Arc::make_mut(&mut view.acknowledged_idle_stops).insert(key.clone(), episode);
            view.acknowledgement_revision = view.acknowledgement_revision.wrapping_add(1);
            view.diagnostic("idle_stop_ack_applied", json!({"key":key, "episode":episode, "lifetime":"viewer_process", "persisted":false}));
            drop(received);
            if !self.wake_pending.swap(true, Ordering::AcqRel) {
                (self.wake)();
            }
            return true;
        }
        if view.completion_episodes.get(key) != Some(&episode) {
            view.diagnostic(
                "ack_rejected_stale",
                json!({"key":key, "episode":episode, "current_episode":view.stop_episode(key)}),
            );
            return false;
        }
        if let (Some(store), Some(source)) = (&self.store, verified_source(view)) {
            match store.acknowledge(&source, key, episode) {
                Ok(()) => {
                    view.persistence_error = None;
                    view.diagnostic("ack_saved", json!({"key":key, "episode":episode}));
                }
                Err(error) => {
                    storage_error(view, error);
                    received
                        .unsaved_acknowledgements
                        .insert(key.clone(), episode);
                }
            }
        } else if self.store.is_some() {
            storage_error(
                view,
                "Coordinator identity unavailable; dismissal lasts only for this launch",
            );
        }
        let view = &mut received.view;
        Arc::make_mut(&mut view.acknowledged_completions).insert(key.clone(), episode);
        view.acknowledgement_revision = view.acknowledgement_revision.wrapping_add(1);
        view.diagnostic("ack_applied", json!({"key":key, "episode":episode, "persisted":view.persistence_error.is_none() && self.store.is_some()}));
        drop(received);
        if !self.wake_pending.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
        true
    }
}
async fn connect(
    port: u16,
) -> Result<
    (
        LocalObserverClient<tonic::transport::Channel>,
        crate::pb::ObserverInfo,
    ),
    String,
> {
    let channel = Endpoint::from_shared(format!("http://127.0.0.1:{port}"))
        .unwrap()
        .connect_timeout(CONNECT_TIMEOUT)
        .connect()
        .await
        .map_err(|_| "Local daemon unavailable — start herdr-mesh or check --port".to_string())?;
    let mut client =
        LocalObserverClient::new(channel).max_decoding_message_size(MAX_SNAPSHOT_BYTES);
    let info = tokio::time::timeout(CONNECT_TIMEOUT, client.get_info(()))
        .await
        .map_err(|_| "Local endpoint did not answer the observer handshake".to_string())?
        .map_err(|e| match e.code() {
            Code::ResourceExhausted => {
                "Observer information capacity reached; retrying".to_string()
            }
            Code::DeadlineExceeded => {
                "Local endpoint did not answer the observer handshake".to_string()
            }
            Code::Unavailable => "Local observer temporarily unavailable; retrying".to_string(),
            _ => "Incompatible local endpoint — expected LocalObserver API 1".to_string(),
        })?
        .into_inner();
    if info.api_version != 1 {
        return Err(format!(
            "Incompatible observer API {} — expected 1",
            info.api_version
        ));
    }
    Ok((client, info))
}
fn rpc_error(code: Code) -> String {
    match code {
        Code::Unimplemented => "Incompatible endpoint — WatchNodes unavailable",
        Code::ResourceExhausted | Code::OutOfRange => {
            "Observer capacity or snapshot size limit reached"
        }
        Code::DeadlineExceeded => "Observation stream renewed; reconnecting",
        _ => "Local daemon reachable; mesh observation stream unavailable",
    }
    .into()
}
async fn observe(port: u16, shared: &Shared) -> Result<(), String> {
    shared.diagnostic(
        "connect_attempt",
        json!({"endpoint":format!("127.0.0.1:{port}")}),
    );
    let (mut client, info) = connect(port).await?;
    shared.update(|v| {
        v.daemon = crate::projection::label(&info.daemon_name);
        v.epoch = v.epoch.wrapping_add(1);
        v.live = false;
        v.status = "Local daemon connected; waiting for mesh snapshot…".into();
    });
    shared.diagnostic("connected", json!({"api_version":info.api_version, "daemon":crate::projection::label(&info.daemon_name), "handshake_source":info.coordinator.as_ref().map(|c| crate::projection::label(&c.instance_id))}));
    let mut request = Request::new(());
    request.set_timeout(Duration::from_secs(270));
    let mut stream = tokio::time::timeout(CONNECT_TIMEOUT, client.watch_nodes(request))
        .await
        .map_err(|_| "Local daemon reachable; mesh stream did not start".to_string())?
        .map_err(|e| rpc_error(e.code()))?
        .into_inner();
    let mut first = true;
    loop {
        let next = if first {
            tokio::time::timeout(FIRST_SNAPSHOT_TIMEOUT, stream.message())
                .await
                .map_err(|_| "Local daemon reachable; first mesh snapshot timed out".to_string())?
        } else {
            stream.message().await
        };
        let snapshot = next
            .map_err(|e| rpc_error(e.code()))?
            .ok_or_else(|| "Observation stream ended; reconnecting".to_string())?;
        let mut scene = project(snapshot).map_err(|_| {
            shared.diagnostic(
                "snapshot_rejected",
                json!({"reason":"invalid replacement snapshot"}),
            );
            "Invalid replacement snapshot — retaining last good observations".to_string()
        })?;
        scene.coordinator = Some(coordinator(info.coordinator.as_ref()));
        let scene = Arc::new(scene);
        shared.update(|v| {
            v.scene = Some(scene);
            v.received = Some(Instant::now());
            v.live = true;
            v.revision = v.revision.wrapping_add(1);
            v.status = "Live mesh observation stream".into();
        });
        first = false;
        // Deliberately no inactivity timeout: WatchNodes emits only changes.
    }
}
pub async fn run(port: u16, shared: Arc<Shared>, mut shutdown: watch::Receiver<bool>) {
    let mut delay = 1;
    loop {
        let started = Instant::now();
        let result = tokio::select! {
            result=observe(port,&shared)=>result,
            _=shutdown.changed()=>{
                shared.diagnostic("observer_stopped", json!({}));
                return;
            },
        };
        shared.update(|v| {
            v.live = false;
            v.status = result.err().unwrap_or_else(|| "Reconnecting".into());
        });
        if started.elapsed() > Duration::from_secs(10) {
            delay = 1;
        }
        {
            let received = shared.received.lock().unwrap();
            received.view.diagnostic(
                "disconnected",
                json!({"reason":received.view.status, "retry_seconds":delay}),
            );
        }
        tokio::select! { _=tokio::time::sleep(Duration::from_secs(delay))=>{}, _=shutdown.changed()=>return }
        delay = (delay * 2).min(8);
    }
}

/// Headless wire-contract check; shares the viewer's handshake and decode path.
pub async fn check(port: u16) -> Result<Scene, String> {
    let (mut client, info) = connect(port).await?;
    let mut request = Request::new(());
    request.set_timeout(FIRST_SNAPSHOT_TIMEOUT);
    let mut stream = tokio::time::timeout(CONNECT_TIMEOUT, client.watch_nodes(request))
        .await
        .map_err(|_| "Stream startup timed out".to_string())?
        .map_err(|e| rpc_error(e.code()))?
        .into_inner();
    let snapshot = tokio::time::timeout(FIRST_SNAPSHOT_TIMEOUT, stream.message())
        .await
        .map_err(|_| "First snapshot timed out".to_string())?
        .map_err(|e| rpc_error(e.code()))?
        .ok_or_else(|| "Empty stream".to_string())?;
    let mut scene = project(snapshot)?;
    scene.coordinator = Some(coordinator(info.coordinator.as_ref()));
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pb::{HerdrEntity, HerdrState, NodeList, NodeView, ServerInfo};

    fn scene(status: &str, source: &str) -> Arc<Scene> {
        let mut scene = project(NodeList {
            nodes: ["node", "other"]
                .into_iter()
                .map(|id| NodeView {
                    instance_id: id.into(),
                    herdr: Some(HerdrState {
                        agents: vec![HerdrEntity {
                            id: "agent".into(),
                            workspace_id: "workspace".into(),
                            agent_status: if id == "node" { status } else { "done" }.into(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .collect(),
        })
        .unwrap();
        scene.coordinator = Some(coordinator(Some(&ServerInfo {
            instance_id: source.into(),
            ..Default::default()
        })));
        Arc::new(scene)
    }
    fn accept(shared: &Shared, scene: Arc<Scene>) {
        shared.update(|view| {
            view.scene = Some(scene);
            view.revision += 1;
            view.live = true;
        });
    }
    #[test]
    fn idle_stops_track_coalesced_work_and_rearm_each_cycle_before_rendering() {
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = wakes.clone();
        let shared = Shared::new(move || {
            count.fetch_add(1, Ordering::Relaxed);
        });
        accept(&shared, scene("working", "source"));
        accept(&shared, scene("idle", "source")); // No read/render between observations.
        assert_eq!(wakes.load(Ordering::Relaxed), 1);
        let first = shared.read();
        assert_eq!(first.idle_stop_episodes.len(), 1);
        assert_eq!(first.completion_episodes.len(), 1); // Other node is genuinely Done.
        let key = first.idle_stop_episodes.keys().next().unwrap().clone();
        let episode = first.idle_stop_episodes[&key];
        assert!(shared.acknowledge(&key, episode));
        accept(&shared, scene("idle", "source"));
        assert!(shared.read().stop_acknowledged(&key, episode));
        accept(&shared, scene("working", "source"));
        accept(&shared, scene("idle", "source"));
        let second = shared.read();
        assert_ne!(second.idle_stop_episodes[&key], episode);
        assert!(second.acknowledged_idle_stops.is_empty());
        assert!(!shared.acknowledge(&key, episode));
        assert!(!second.completion_episodes.contains_key(&key));
    }
    #[test]
    fn idle_baselines_and_non_working_transitions_do_not_invent_a_work_stop() {
        for before in ["idle", "blocked", "done", "unknown"] {
            let shared = Shared::new(|| {});
            accept(&shared, scene(before, "source"));
            accept(&shared, scene("idle", "source"));
            assert!(
                shared.read().idle_stop_episodes.is_empty(),
                "{before} -> Idle"
            );
        }
        let shared = Shared::new(|| {});
        accept(&shared, scene("working", "old"));
        accept(&shared, scene("idle", "new"));
        assert!(shared.read().idle_stop_episodes.is_empty());
        accept(&shared, scene("working", "new"));
        let mut missing = scene("idle", "new");
        Arc::get_mut(&mut missing)
            .unwrap()
            .nodes
            .retain(|n| n.key[1] != "node");
        accept(&shared, missing);
        accept(&shared, scene("idle", "new"));
        assert!(shared.read().idle_stop_episodes.is_empty());
    }
    #[test]
    fn idle_stop_dismissal_does_not_change_completed_disk_records_or_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ack.bin");
        let store = Store::at(path.clone());
        let shared = Shared::with_store(|| {}, Some(store.clone()), 100);
        accept(&shared, scene("working", "source"));
        let v = shared.read();
        let done_key = v.completion_episodes.keys().next().unwrap().clone();
        assert!(shared.acknowledge(&done_key, v.completion_episodes[&done_key]));
        let saved = std::fs::read(&path).unwrap();
        accept(&shared, scene("idle", "source"));
        let v = shared.read();
        let key = v.idle_stop_episodes.keys().next().unwrap().clone();
        assert!(shared.acknowledge(&key, v.idle_stop_episodes[&key]));
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        let restarted = Shared::with_store(|| {}, Some(store), 200);
        accept(&restarted, scene("idle", "source"));
        let v = restarted.read();
        assert!(v.idle_stop_episodes.is_empty());
        assert!(v.acknowledged_idle_stops.is_empty());
        assert_eq!(v.acknowledged_completions[&done_key], 101);
    }
    #[test]
    fn idle_stops_preserve_verified_reconnect_but_not_unknown_source_baselines() {
        for verified in [true, false] {
            let shared = Shared::new(|| {});
            let input = |status| {
                let mut s = scene(status, "source");
                if !verified {
                    Arc::get_mut(&mut s)
                        .unwrap()
                        .coordinator
                        .as_mut()
                        .unwrap()
                        .status = "unknown".into();
                }
                s
            };
            accept(&shared, input("working"));
            accept(&shared, input("idle"));
            let old = shared.read().idle_stop_episodes;
            assert_eq!(old.len(), 1);
            shared.update(|v| {
                v.live = false;
                v.epoch += 1;
            });
            accept(&shared, input("idle"));
            let new = shared.read().idle_stop_episodes;
            if verified {
                assert_eq!(old, new);
            } else {
                assert!(new.is_empty());
            }
        }
    }
    #[test]
    fn idle_stops_are_scoped_independent_and_retire_on_any_departure_from_idle() {
        let shared = Shared::new(|| {});
        let both = |state| {
            let mut s = scene(state, "source");
            Arc::get_mut(&mut s).unwrap().nodes[1].children[0].children[0].children[0].status =
                state.into();
            s
        };
        accept(&shared, both("working"));
        accept(&shared, both("idle"));
        let v = shared.read();
        assert_eq!(v.idle_stop_episodes.len(), 2);
        assert!(v.completion_episodes.is_empty());
        let entries: Vec<_> = v.idle_stop_episodes.iter().collect();
        assert_eq!(entries[0].0.last(), entries[1].0.last());
        assert_ne!(entries[0].1, entries[1].1);
        let (dismissed, episode) = entries[0];
        assert!(shared.acknowledge(dismissed, *episode));
        let v = shared.read();
        assert_eq!(v.acknowledged_idle_stops.len(), 1);
        assert!(!v.stop_acknowledged(entries[1].0, *entries[1].1));
        for next in ["working", "blocked", "done", "unknown"] {
            accept(&shared, both(next));
            assert!(
                shared.read().idle_stop_episodes.is_empty(),
                "Idle -> {next}"
            );
            assert!(shared.read().acknowledged_idle_stops.is_empty());
            accept(&shared, both("working"));
            accept(&shared, both("idle"));
            assert_eq!(shared.read().idle_stop_episodes.len(), 2);
        }
        let mut removed = scene("idle", "source");
        Arc::get_mut(&mut removed).unwrap().nodes.clear();
        accept(&shared, removed);
        assert!(shared.read().idle_stop_episodes.is_empty());
        accept(&shared, both("idle"));
        assert!(shared.read().idle_stop_episodes.is_empty());
    }
    #[test]
    fn idle_stop_diagnostics_have_exact_tokens_without_a_saved_claim() {
        let dir = tempfile::tempdir().unwrap();
        let log = Diagnostics::at(dir.path(), 8790).unwrap();
        let shared = Shared::with_store(|| {}, None, u64::MAX - 10);
        shared.attach_diagnostics(log.clone());
        accept(&shared, scene("working", "source"));
        accept(&shared, scene("idle", "source"));
        let v = shared.read();
        let (key, episode) = v.idle_stop_episodes.iter().next().unwrap();
        shared.acknowledge(key, *episode);
        accept(&shared, scene("working", "source"));
        log.flush();
        let records: Vec<Value> = std::fs::read_to_string(log.path())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let episode_text = episode.to_string();
        for name in [
            "idle_stop_created",
            "idle_stop_ack_applied",
            "idle_stop_retired",
        ] {
            assert!(records.iter().any(|r| r["event"] == name
                && r["data"]["episode"].as_str() == Some(episode_text.as_str())));
        }
        assert!(records.iter().any(|r| r["event"] == "agent_state"
            && r["data"]["idle_stop_episode"].as_str() == Some(episode_text.as_str())));
        assert!(!records.iter().any(|r| r["event"] == "ack_saved"));
    }
    #[test]
    fn diagnostics_follow_received_episodes_ack_restore_and_retirement_without_consuming_wakes() {
        let dir = tempfile::tempdir().unwrap();
        let log = Diagnostics::at(&dir.path().join("logs"), 8790).unwrap();
        let store = Store::at(dir.path().join("ack.bin"));
        let first = Shared::with_store(|| {}, Some(store.clone()), 100);
        first.attach_diagnostics(log.clone());
        accept(&first, scene("working", "source"));
        assert!(first.wake_pending.load(Ordering::Relaxed));
        first.diagnostic("connected", json!({}));
        assert!(
            first.wake_pending.load(Ordering::Relaxed),
            "logging must not consume UI wakeups"
        );
        accept(&first, scene("done", "source"));
        let completed = first.read();
        let key = completed
            .completion_episodes
            .keys()
            .find(|k| k[1] == "node")
            .unwrap()
            .clone();
        let episode = completed.completion_episodes[&key];
        assert!(first.acknowledge(&key, episode));
        let second = Shared::with_store(|| {}, Some(store), 200);
        second.attach_diagnostics(log.clone());
        accept(&second, scene("done", "source"));
        assert_eq!(second.read().acknowledged_completions[&key], episode);
        accept(&second, scene("working", "source"));
        accept(&second, scene("done", "source"));
        assert!(!second.acknowledge(&key, episode));
        let latest = second.read().completion_episodes[&key];
        log.flush();
        let records: Vec<Value> = std::fs::read_to_string(log.path())
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        let agent_events: Vec<_> = records
            .iter()
            .filter(|r| r["data"]["key"] == json!(key))
            .collect();
        for name in [
            "agent_state",
            "completion_created",
            "ack_requested",
            "ack_saved",
            "ack_applied",
            "ack_restored",
            "completion_retired",
            "ack_retired",
            "ack_rejected_stale",
        ] {
            assert!(
                agent_events.iter().any(|r| r["event"] == name),
                "missing {name}"
            );
        }
        assert!(agent_events.iter().any(|r| {
            r["event"] == "completion_created"
                && r["data"]["episode"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    == Some(latest)
        }));
        assert!(
            agent_events
                .iter()
                .all(|r| r["data"]["source"] == json!(["coordinator", "source"]))
        );
        let state_events = agent_events
            .iter()
            .filter(|r| r["event"] == "agent_state")
            .count();
        accept(&second, scene("done", "source"));
        log.flush();
        let unchanged = std::fs::read_to_string(log.path()).unwrap();
        assert_eq!(
            unchanged
                .lines()
                .map(|s| serde_json::from_str::<Value>(s).unwrap())
                .filter(|r| r["event"] == "agent_state" && r["data"]["key"] == json!(key))
                .count(),
            state_events
        );
    }
    #[test]
    fn local_input_preserved_during_failed_startup_load_is_not_logged_as_disk_restoration() {
        let dir = tempfile::tempdir().unwrap();
        let log = Diagnostics::at(&dir.path().join("logs"), 8790).unwrap();
        let path = dir.path().join("ack.bin");
        let lock = std::fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .unwrap();
        lock.try_lock().unwrap();
        let shared = Shared::with_store(|| {}, Some(Store::at(path)), 100);
        shared.attach_diagnostics(log.clone());
        accept(&shared, scene("done", "source"));
        let view = shared.read();
        let key = view.completion_episodes.keys().next().unwrap().clone();
        let episode = view.completion_episodes[&key];
        assert!(shared.acknowledge(&key, episode));
        assert!(shared.read().persistence_error.is_some());
        drop(lock);
        accept(&shared, scene("done", "source"));
        let saved = shared.read();
        assert_eq!(saved.acknowledged_completions[&key], episode);
        assert!(saved.persistence_error.is_none());
        log.flush();
        let records: Vec<Value> = std::fs::read_to_string(log.path())
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert!(records.iter().any(|r| r["event"] == "ack_restore_result"
            && r["data"]["matched"] == 0
            && r["data"]["file_present"] == false));
        assert!(
            records
                .iter()
                .any(|r| r["event"] == "ack_applied" && r["data"]["persisted"] == false)
        );
        assert!(
            records
                .iter()
                .any(|r| r["event"] == "ack_saved_retry" && r["data"]["key"] == json!(key))
        );
        assert!(
            !records
                .iter()
                .any(|r| r["event"] == "ack_restored" && r["data"]["key"] == json!(key))
        );
    }
    #[test]
    fn restart_diagnostics_explain_missing_file_source_state_membership_and_delayed_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let log = Diagnostics::at(&dir.path().join("logs"), 8790).unwrap();
        let path = dir.path().join("ack.bin");
        let store = Store::at(path.clone());
        let first = Shared::with_store(|| {}, Some(store.clone()), 100);
        first.attach_diagnostics(log.clone());
        accept(&first, scene("done", "source"));
        let initial = first.read();
        let key = initial
            .completion_episodes
            .keys()
            .find(|k| k[1] == "node")
            .unwrap()
            .clone();
        assert!(first.acknowledge(&key, initial.completion_episodes[&key]));
        drop(first);

        // A namespace mismatch must be explained without modifying the other source.
        let other = Shared::with_store(|| {}, Some(store.clone()), 200);
        other.attach_diagnostics(log.clone());
        accept(&other, scene("done", "another-source"));
        assert!(other.read().acknowledged_completions.is_empty());
        drop(other);

        let unchanged = Shared::with_store(|| {}, Some(store.clone()), 300);
        unchanged.attach_diagnostics(log.clone());
        accept(&unchanged, scene("done", "source"));
        assert_eq!(
            unchanged.read().acknowledged_completions[&key],
            initial.completion_episodes[&key]
        );
        drop(unchanged);

        let not_done = Shared::with_store(|| {}, Some(store.clone()), 400);
        not_done.attach_diagnostics(log.clone());
        accept(&not_done, scene("working", "source"));
        assert!(not_done.read().acknowledged_completions.is_empty());
        drop(not_done);

        store
            .acknowledge(&vec!["coordinator".into(), "source".into()], &key, 500)
            .unwrap();
        let missing = Shared::with_store(|| {}, Some(store.clone()), 600);
        missing.attach_diagnostics(log.clone());
        let mut absent = scene("done", "source");
        Arc::get_mut(&mut absent)
            .unwrap()
            .nodes
            .retain(|n| n.key[1] != "node");
        accept(&missing, absent);
        assert!(missing.read().acknowledged_completions.is_empty());
        drop(missing);

        store
            .acknowledge(&vec!["coordinator".into(), "source".into()], &key, 700)
            .unwrap();
        let lock = std::fs::File::options()
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .unwrap();
        lock.try_lock().unwrap();
        let delayed = Shared::with_store(|| {}, Some(store), 800);
        delayed.attach_diagnostics(log.clone());
        accept(&delayed, scene("done", "source"));
        accept(&delayed, scene("working", "source"));
        accept(&delayed, scene("done", "source"));
        drop(lock);
        accept(&delayed, scene("done", "source"));
        assert!(delayed.read().acknowledged_completions.is_empty());
        assert!(delayed.read().persistence_error.is_none());

        let unavailable = Shared::new(|| {});
        unavailable.attach_diagnostics(log.clone());
        accept(&unavailable, scene("done", "source"));
        let unknown =
            Shared::with_store(|| {}, Some(Store::at(dir.path().join("unknown.bin"))), 900);
        unknown.attach_diagnostics(log.clone());
        let mut unverified = scene("done", "source");
        Arc::get_mut(&mut unverified).unwrap().coordinator = Some(coordinator(None));
        accept(&unknown, unverified);

        log.flush();
        let records: Vec<Value> = std::fs::read_to_string(log.path())
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert!(records.iter().any(|r| r["event"] == "ack_restore_result"
            && r["data"]["file_present"] == false
            && r["data"]["matched"] == 0));
        assert!(
            records
                .iter()
                .any(|r| r["event"] == "ack_restore_source_mismatch"
                    && r["data"]["saved_source"] == json!(["coordinator", "source"])
                    && r["data"]["records"] == 1)
        );
        let initial_episode = initial.completion_episodes[&key].to_string();
        assert!(records.iter().any(|r| r["event"] == "ack_restored"
            && r["data"]["key"] == json!(key)
            && r["data"]["episode"] == initial_episode));
        for reason in [
            "agent_not_done",
            "agent_missing",
            "completion_changed_during_restore",
        ] {
            assert!(
                records.iter().any(|r| r["event"] == "ack_restore_excluded"
                    && r["data"]["key"] == json!(key)
                    && r["data"]["reason"] == reason
                    && r["data"]["retired_from_disk"] == true),
                "missing {reason}"
            );
        }
        assert!(
            records
                .iter()
                .any(|r| r["event"] == "ack_restore_failed"
                    && r["data"]["error_kind"] == "WouldBlock")
        );
        for reason in ["storage_unavailable", "no_verified_source"] {
            assert!(
                records
                    .iter()
                    .any(|r| r["event"] == "ack_restore_unavailable"
                        && r["data"]["reason"] == reason),
                "missing {reason}"
            );
        }
        assert!(
            !records.iter().any(|r| r["event"] == "diagnostic_loss"
                || r["dropped_before"].as_u64().unwrap_or(0) > 0)
        );
    }
    #[test]
    fn completion_instances_advance_even_when_ui_never_reads_the_working_snapshot() {
        let shared = Shared::new(|| {});
        accept(&shared, scene("done", "source"));
        let first = shared.read().completion_episodes;
        assert_eq!(first.len(), 2);
        accept(&shared, scene("working", "source"));
        accept(&shared, scene("done", "source"));
        let second = shared.read().completion_episodes;
        let key = first.keys().find(|k| k[1] == "node").unwrap();
        let other = first.keys().find(|k| k[1] == "other").unwrap();
        assert_ne!(first[key], second[key]);
        assert_eq!(
            first[other], second[other],
            "same agent ID on another node retains its own episode"
        );
        accept(&shared, scene("done", "source"));
        assert_eq!(*shared.read().completion_episodes, *second);
    }
    #[test]
    fn completion_instances_survive_reconnect_but_not_removal_or_source_replacement() {
        let shared = Shared::new(|| {});
        accept(&shared, scene("done", "one"));
        let first = shared.read().completion_episodes;
        shared.update(|v| {
            v.live = false;
            v.epoch += 1;
        });
        accept(&shared, scene("done", "one"));
        assert_eq!(*shared.read().completion_episodes, *first);
        accept(&shared, Arc::new(Scene::default()));
        assert!(shared.read().completion_episodes.is_empty());
        accept(&shared, scene("done", "one"));
        let reappeared = shared.read().completion_episodes;
        let key = first.keys().next().unwrap();
        assert_ne!(first[key], reappeared[key]);
        accept(&shared, scene("done", "two"));
        assert_ne!(reappeared[key], shared.read().completion_episodes[key]);
    }
    #[test]
    fn dismissal_survives_real_storage_reload_and_received_work_retires_it_before_rendering() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::at(dir.path().join("dismissals.bin"));
        let first = Shared::with_store(|| {}, Some(store.clone()), 100);
        accept(&first, scene("done", "source"));
        let initial = first.read();
        let key = initial
            .completion_episodes
            .keys()
            .find(|k| k[1] == "node")
            .unwrap()
            .clone();
        let other = initial
            .completion_episodes
            .keys()
            .find(|k| k[1] == "other")
            .unwrap()
            .clone();
        let episode = initial.completion_episodes[&key];
        assert!(first.acknowledge(&key, episode));
        drop(first);

        let second = Shared::with_store(|| {}, Some(store.clone()), 200);
        accept(&second, scene("done", "source"));
        let restored = second.read();
        assert_eq!(restored.completion_episodes[&key], episode);
        assert_eq!(
            *restored.acknowledged_completions,
            BTreeMap::from([(key.clone(), episode)])
        );
        assert!(!restored.acknowledged_completions.contains_key(&other));
        second.update(|v| {
            v.live = false;
            v.epoch += 1;
        });
        accept(&second, scene("done", "source"));
        assert_eq!(second.read().acknowledged_completions[&key], episode);

        // No UI read of Working; even closing here must retire the saved dismissal.
        accept(&second, scene("working", "source"));
        drop(second);
        let third = Shared::with_store(|| {}, Some(store.clone()), 300);
        accept(&third, scene("done", "source"));
        let later = third.read();
        assert!(later.acknowledged_completions.is_empty());
        assert_ne!(later.completion_episodes[&key], episode);
        assert!(
            !third.acknowledge(&key, episode),
            "stale click cannot acknowledge the newer completion"
        );
        assert!(third.acknowledge(&key, later.completion_episodes[&key]));
        accept(&third, scene("working", "source"));
        accept(&third, scene("done", "source"));
        assert!(third.read().acknowledged_completions.is_empty());
        let fourth = Shared::with_store(|| {}, Some(store), 400);
        accept(&fourth, scene("done", "source"));
        assert!(fourth.read().acknowledged_completions.is_empty());
    }
    #[test]
    fn saved_acknowledgements_do_not_leak_to_another_source_or_absent_agent() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::at(dir.path().join("dismissals.bin"));
        let a = Shared::with_store(|| {}, Some(store.clone()), 100);
        accept(&a, scene("done", "one"));
        let initial = a.read();
        let key = initial.completion_episodes.keys().next().unwrap().clone();
        assert!(a.acknowledge(&key, initial.completion_episodes[&key]));
        let b = Shared::with_store(|| {}, Some(store), 200);
        accept(&b, scene("done", "two"));
        assert!(b.read().acknowledged_completions.is_empty());
        accept(&b, scene("working", "one"));
        // The other node can be Done, but only a matching scoped agent restores.
        if key[1] == "node" {
            assert!(b.read().acknowledged_completions.is_empty());
        }
        accept(&b, scene("done", "one"));
        assert!(!b.read().acknowledged_completions.contains_key(&key));
    }
    #[test]
    fn storage_contention_retries_without_suppressing_new_completions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dismissals.bin");
        let store = Store::at(path.clone());
        let shared = Shared::with_store(|| {}, Some(store.clone()), 100);
        accept(&shared, scene("done", "source"));
        let initial = shared.read();
        let key = initial
            .completion_episodes
            .keys()
            .find(|k| k[1] == "node")
            .unwrap()
            .clone();
        let episode = initial.completion_episodes[&key];
        let lock = std::fs::File::options()
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .unwrap();
        lock.try_lock().unwrap();
        assert!(shared.acknowledge(&key, episode));
        assert!(shared.read().persistence_error.is_some());
        drop(lock);
        accept(&shared, scene("done", "source"));
        assert!(shared.read().persistence_error.is_none());
        let restarted = Shared::with_store(|| {}, Some(store.clone()), 200);
        accept(&restarted, scene("done", "source"));
        assert_eq!(restarted.read().acknowledged_completions[&key], episode);
        drop(restarted);

        let lock = std::fs::File::options()
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .unwrap();
        lock.try_lock().unwrap();
        accept(&shared, scene("working", "source"));
        accept(&shared, scene("done", "source"));
        let latest = shared.read();
        assert!(latest.persistence_error.is_some());
        assert_ne!(latest.completion_episodes[&key], episode);
        // A pending failed retirement cannot hide the new instance.
        assert_ne!(
            latest.acknowledged_completions[&key],
            latest.completion_episodes[&key]
        );
        drop(lock);
        accept(&shared, scene("done", "source"));
        assert!(shared.read().persistence_error.is_none());
        let restarted = Shared::with_store(|| {}, Some(store), 300);
        accept(&restarted, scene("done", "source"));
        assert!(restarted.read().acknowledged_completions.is_empty());
    }
    #[test]
    fn first_snapshot_removal_retires_saved_acknowledgement_and_ports_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::at(dir.path().join("port-one.bin"));
        let first = Shared::with_store(|| {}, Some(store.clone()), 100);
        accept(&first, scene("done", "source"));
        let initial = first.read();
        let key = initial.completion_episodes.keys().next().unwrap().clone();
        first.acknowledge(&key, initial.completion_episodes[&key]);
        let other =
            Shared::with_store(|| {}, Some(Store::at(dir.path().join("port-two.bin"))), 200);
        accept(&other, scene("done", "source"));
        assert!(other.read().acknowledged_completions.is_empty());
        let empty = Arc::new(Scene {
            coordinator: Some(coordinator(Some(&ServerInfo {
                instance_id: "source".into(),
                ..Default::default()
            }))),
            ..Default::default()
        });
        let restarted = Shared::with_store(|| {}, Some(store.clone()), 300);
        accept(&restarted, empty);
        assert!(restarted.read().acknowledged_completions.is_empty());
        drop(restarted);
        let later = Shared::with_store(|| {}, Some(store), 400);
        accept(&later, scene("done", "source"));
        assert!(later.read().acknowledged_completions.is_empty());
    }
    #[test]
    fn delayed_startup_load_cannot_restore_an_acknowledgement_after_received_work() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dismissals.bin");
        let store = Store::at(path.clone());
        let first = Shared::with_store(|| {}, Some(store.clone()), 100);
        accept(&first, scene("done", "source"));
        let initial = first.read();
        let key = initial
            .completion_episodes
            .keys()
            .find(|k| k[1] == "node")
            .unwrap()
            .clone();
        first.acknowledge(&key, initial.completion_episodes[&key]);
        drop(first);
        let lock = std::fs::File::options()
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .unwrap();
        lock.try_lock().unwrap();
        let restarted = Shared::with_store(|| {}, Some(store.clone()), 200);
        accept(&restarted, scene("done", "source"));
        assert!(restarted.read().persistence_error.is_some());
        accept(&restarted, scene("working", "source"));
        accept(&restarted, scene("done", "source"));
        drop(lock);
        accept(&restarted, scene("done", "source"));
        assert!(restarted.read().persistence_error.is_none());
        assert!(restarted.read().acknowledged_completions.is_empty());
        let later = Shared::with_store(|| {}, Some(store), 300);
        accept(&later, scene("done", "source"));
        assert!(later.read().acknowledged_completions.is_empty());
    }
}
