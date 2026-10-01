use herdr_mesh_visualizer::{
    client::{self, Shared},
    pb::{
        self,
        local_observer_server::{LocalObserver, LocalObserverServer},
    },
};
use std::{
    collections::VecDeque,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_stream::{
    Stream,
    wrappers::{ReceiverStream, TcpListenerStream},
};
use tonic::{Request, Response, Status};

type SnapshotReceiver = mpsc::Receiver<Result<pb::NodeList, Status>>;
type FixtureStreams = Arc<Mutex<VecDeque<SnapshotReceiver>>>;

struct Service {
    api: u32,
    receiver: FixtureStreams,
    calls: Arc<AtomicUsize>,
}
#[tonic::async_trait]
impl LocalObserver for Service {
    async fn get_info(&self, _: Request<()>) -> Result<Response<pb::ObserverInfo>, Status> {
        Ok(Response::new(pb::ObserverInfo {
            api_version: self.api,
            daemon_name: "fixture".into(),
        }))
    }
    type WatchNodesStream = Pin<Box<dyn Stream<Item = Result<pb::NodeList, Status>> + Send>>;
    async fn watch_nodes(
        &self,
        _: Request<()>,
    ) -> Result<Response<Self::WatchNodesStream>, Status> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let rx = self
            .receiver
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| Status::unavailable("fixture exhausted"))?;
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}
struct Fixture {
    port: u16,
    tx: mpsc::Sender<Result<pb::NodeList, Status>>,
    calls: Arc<AtomicUsize>,
    receiver: FixtureStreams,
    stop: Option<oneshot::Sender<()>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
async fn fixture(api: u32) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel(4);
    let calls = Arc::new(AtomicUsize::new(0));
    let (stop, stopped) = oneshot::channel();
    let receiver = Arc::new(Mutex::new(VecDeque::from([rx])));
    let service = Service {
        api,
        receiver: receiver.clone(),
        calls: calls.clone(),
    };
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(
                LocalObserverServer::new(service)
                    .max_encoding_message_size(client::MAX_SNAPSHOT_BYTES + 1024),
            )
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    Fixture {
        port,
        tx,
        calls,
        receiver,
        stop: Some(stop),
    }
}

#[tokio::test]
async fn reconnect_after_lease_expiry_replaces_the_complete_scene() {
    let f = fixture(1).await;
    let (next_tx, next_rx) = mpsc::channel(4);
    f.receiver.lock().unwrap().push_back(next_rx);
    f.tx.send(Ok(pb::NodeList {
        nodes: vec![node("old")],
    }))
    .await
    .unwrap();
    next_tx
        .send(Ok(pb::NodeList {
            nodes: vec![node("new")],
        }))
        .await
        .unwrap();
    let shared = Shared::new(|| {});
    let (stop, rx) = watch::channel(false);
    let worker = tokio::spawn(client::run(f.port, shared.clone(), rx));
    until(|| shared.read().live).await;
    f.tx.send(Err(Status::deadline_exceeded("lease expired")))
        .await
        .unwrap();
    until(|| !shared.read().live).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(2)).await;
    // Resume before waiting for real TCP/HTTP2 I/O. A paused clock can otherwise
    // expire network deadlines before the OS delivers its response.
    tokio::time::resume();
    until(|| shared.read().live && f.calls.load(Ordering::SeqCst) == 2).await;
    let scene = shared.read().scene.unwrap();
    assert_eq!(scene.nodes.len(), 1);
    assert!(scene.nodes[0].key.contains(&"new".to_string()));
    stop.send(true).unwrap();
    worker.await.unwrap();
}
fn node(id: &str) -> pb::NodeView {
    pb::NodeView {
        instance_id: id.into(),
        connected: true,
        ..Default::default()
    }
}
async fn until(predicate: impl Fn() -> bool) {
    until_for(Duration::from_secs(8), predicate).await;
}
async fn until_for(timeout: Duration, predicate: impl Fn() -> bool) {
    // An iteration budget measures CPU speed, not network progress. Use a real
    // bounded deadline and yield time to sockets on slower CI platforms.
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if predicate() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition not observed");
}
#[tokio::test]
async fn quiet_stream_retains_live_and_cancellation_joins() {
    let f = fixture(1).await;
    f.tx.send(Ok(pb::NodeList {
        nodes: vec![node("n")],
    }))
    .await
    .unwrap();
    let shared = Shared::new(|| {});
    let (stop, rx) = watch::channel(false);
    let worker = tokio::spawn(client::run(f.port, shared.clone(), rx));
    until(|| shared.read().live).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(35)).await;
    tokio::time::resume();
    tokio::task::yield_now().await;
    assert!(shared.read().live, "quiet healthy watch must remain live");
    assert_eq!(f.calls.load(Ordering::SeqCst), 1);
    stop.send(true).unwrap();
    worker.await.unwrap();
}
#[tokio::test]
async fn malformed_replacement_retains_last_good_scene() {
    let f = fixture(1).await;
    f.tx.send(Ok(pb::NodeList {
        nodes: vec![node("n")],
    }))
    .await
    .unwrap();
    let shared = Shared::new(|| {});
    let (stop, rx) = watch::channel(false);
    let worker = tokio::spawn(client::run(f.port, shared.clone(), rx));
    until(|| shared.read().live).await;
    let original = shared.read().scene.unwrap();
    f.tx.send(Ok(pb::NodeList {
        nodes: vec![node("n"), node("n")],
    }))
    .await
    .unwrap();
    until(|| shared.read().status.contains("Invalid replacement")).await;
    let view = shared.read();
    assert!(!view.live);
    assert!(Arc::ptr_eq(&original, &view.scene.unwrap()));
    stop.send(true).unwrap();
    worker.await.unwrap();
}
#[tokio::test]
async fn missing_first_snapshot_has_a_deadline() {
    let f = fixture(1).await;
    let shared = Shared::new(|| {});
    let (stop, rx) = watch::channel(false);
    let worker = tokio::spawn(client::run(f.port, shared.clone(), rx));
    until(|| f.calls.load(Ordering::SeqCst) == 1).await;
    // Exercise the real socket/first-snapshot timeout: sleeping a fixed 20 ms
    // did not prove headers had arrived before a synthetic clock jump.
    until_for(Duration::from_secs(15), || {
        shared
            .read()
            .status
            .contains("first mesh snapshot timed out")
    })
    .await;
    assert!(shared.read().scene.is_none());
    stop.send(true).unwrap();
    worker.await.unwrap();
}
#[tokio::test]
async fn wrong_api_and_oversized_snapshot_are_rejected() {
    let f = fixture(2).await;
    assert!(
        client::check(f.port)
            .await
            .unwrap_err()
            .contains("Incompatible")
    );
    let f = fixture(1).await;
    let mut n = node("n");
    n.hostname = "x".repeat(client::MAX_SNAPSHOT_BYTES);
    f.tx.send(Ok(pb::NodeList { nodes: vec![n] }))
        .await
        .unwrap();
    assert!(
        client::check(f.port)
            .await
            .unwrap_err()
            .contains("size limit")
    );
}
