use crate::{
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
        }
    }
}
pub struct Shared {
    received: Mutex<Received>,
    wake_pending: AtomicBool,
    wake: Box<dyn Fn() + Send + Sync>,
}
#[derive(Default)]
struct Received {
    view: View,
    completion_serial: u64,
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
impl Shared {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            received: Mutex::new(Received::default()),
            wake_pending: AtomicBool::new(false),
            wake: Box::new(wake),
        })
    }
    pub fn read(&self) -> View {
        self.wake_pending.store(false, Ordering::Release);
        self.received.lock().unwrap().view.clone()
    }
    fn update(&self, f: impl FnOnce(&mut View)) {
        {
            let mut received = self.received.lock().unwrap();
            let Received {
                view,
                completion_serial,
            } = &mut *received;
            let revision = view.revision;
            let source = view
                .scene
                .as_ref()
                .and_then(|s| s.coordinator.as_ref())
                .map(|c| c.key.clone());
            f(view);
            if view.revision != revision {
                let mut current = BTreeMap::new();
                if let Some(scene) = &view.scene {
                    let same_source = source == scene.coordinator.as_ref().map(|c| c.key.clone());
                    let previous = same_source.then_some(view.completion_episodes.as_ref());
                    for node in &scene.nodes {
                        completions(node, previous, completion_serial, &mut current);
                    }
                }
                // Keep only current completions; Working/removal retires the prior episode.
                view.completion_episodes = Arc::new(current);
            }
        }
        if !self.wake_pending.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
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
    let (mut client, info) = connect(port).await?;
    shared.update(|v| {
        v.daemon = crate::projection::label(&info.daemon_name);
        v.epoch = v.epoch.wrapping_add(1);
        v.live = false;
        v.status = "Local daemon connected; waiting for mesh snapshot…".into();
    });
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
            _=shutdown.changed()=>return,
        };
        shared.update(|v| {
            v.live = false;
            v.status = result.err().unwrap_or_else(|| "Reconnecting".into());
        });
        if started.elapsed() > Duration::from_secs(10) {
            delay = 1;
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
}
