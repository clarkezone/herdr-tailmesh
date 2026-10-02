use crate::{
    pb::local_observer_client::LocalObserverClient,
    projection::{Scene, project},
};
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
}
impl Default for View {
    fn default() -> Self {
        Self {
            status: "Connecting to local daemon…".into(),
            live: false,
            daemon: String::new(),
            scene: None,
            received: None,
        }
    }
}
pub struct Shared {
    view: Mutex<View>,
    wake_pending: AtomicBool,
    wake: Box<dyn Fn() + Send + Sync>,
}
impl Shared {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            view: Mutex::new(View::default()),
            wake_pending: AtomicBool::new(false),
            wake: Box::new(wake),
        })
    }
    pub fn read(&self) -> View {
        self.wake_pending.store(false, Ordering::Release);
        self.view.lock().unwrap().clone()
    }
    fn update(&self, f: impl FnOnce(&mut View)) {
        f(&mut self.view.lock().unwrap());
        if !self.wake_pending.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
    }
}
async fn connect(
    port: u16,
) -> Result<(LocalObserverClient<tonic::transport::Channel>, String), String> {
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
        .map_err(|_| "Incompatible local endpoint — expected LocalObserver API 1".to_string())?
        .into_inner();
    if info.api_version != 1 {
        return Err(format!(
            "Incompatible observer API {} — expected 1",
            info.api_version
        ));
    }
    Ok((client, crate::projection::label(&info.daemon_name)))
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
    let (mut client, name) = connect(port).await?;
    shared.update(|v| {
        v.daemon = name;
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
        let scene = Arc::new(project(snapshot).map_err(|_| {
            "Invalid replacement snapshot — retaining last good observations".to_string()
        })?);
        shared.update(|v| {
            v.scene = Some(scene);
            v.received = Some(Instant::now());
            v.live = true;
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
    let (mut client, _) = connect(port).await?;
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
    project(snapshot)
}
