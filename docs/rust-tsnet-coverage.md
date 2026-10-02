# Rust Tailscale coverage for Herdr Tailmesh

Assessment date: 2026-09-29.

This is a historical assessment of the pinned versions below, not a statement
about later upstream releases. By 2026-10-02 the project selected and implemented
a separate Rust viewer using the managed daemon's read-only loopback
LocalObserver service. See [the viewer documentation](../visualizer/README.md).
That approach reuses the existing enrollment; it does not migrate Go tsnet or
embed Go through a new FFI layer. The alternatives below retain the original
assessment context.

**Conclusion: the official Rust implementation covers enough networking primitives for a migration experiment, but does not currently cover all the tsnet behavior this project uses.** The main obstacles are shared-identity self-connections, missing managed status information, automatic HTTPS certificates, identity-state migration, and incomplete coverage of the release architectures. Ordinary peer identification and hostname lookup are available, although they need application adapters.

## Scope and evidence

This review concerns the official [tailscale/tailscale-rs](https://github.com/tailscale/tailscale-rs), published as the Rust crate `tailscale`. It does not assess the independent GeiserX fork or the older Rust `tsnet` crate wrapping Go `libtailscale`.

Local application source was inspected at `237f28fc14da2d54bf45aac84f785d66cdb4df6f`, with Go `tailscale.com v1.102.4`. Production usage is concentrated in `src/internal/transport/tsnet.go`; managed runtime, caller authorization, dashboards, state maintenance, and operator contracts add requirements beyond that wrapper. The archived `experiments/tsnet-spike` is not a production entrypoint.

The Rust review inspected release [v0.6.1](https://github.com/tailscale/tailscale-rs/releases/tag/v0.6.1), commit `d34658bbb2eb4593ae6df959aa93e9ee1e0457c6`, published September 18, and current main [ede063fa7e1f21d36e658170c2e0d5347b944e6b](https://github.com/tailscale/tailscale-rs/commit/ede063fa7e1f21d36e658170c2e0d5347b944e6b), committed September 24. The public Device/Config APIs, node representation, peer tracker, and control StateUpdate conversion examined here are identical between those revisions. Main includes internal refactors and a STUN packet-identification fix; those do not resolve the gaps described below.

This is a source and upstream-issue assessment, not an executed Rust integration test or live-tailnet certification. The pinned files were read through GitHub. No enrollment, tailnet policy, installed binary, or runtime configuration was changed by the assessment.

## Coverage matrix

“Adapter” means the underlying data or mechanism exists but Herdr must implement the equivalent behavior. “Gap” means the inspected implementation does not supply the required functionality. “Unverified” means the evidence is insufficient to claim parity.

| Herdr requirement | Go usage | Rust coverage | Assessment |
|---|---|---|---|
| Embedded userspace tailnet node | `tsnet.Server` | `Device`, Tokio actors, smoltcp stack | Supported primitive; no host tailscaled required |
| Requested hostname and role tags | `Hostname`, `AdvertiseTags` | `requested_hostname`, `requested_tags` | Supported; validate assigned tags after registration |
| Persistent, non-ephemeral enrollment | `Dir`, default non-ephemeral server | Key file / `PersistState`, `ephemeral: false` | Supported for fresh Rust state; existing Go state needs migration |
| Advanced auth-key enrollment | `AuthKey` from configured environment variable | `Device::new(config, Option<String>)` | Supported; preserve Herdr's selected variable and redaction behavior |
| Managed browser enrollment | `UserLogf` login URL, `Up(ctx)` | `is_authorized()` / `AuthState::NotAuthorized(url)` | Adapter for private status publication, waits, cancellation, and readiness |
| IPv4 and IPv6 addresses | `TailscaleIPs()` | `ipv4_addr()`, `ipv6_addr()` | Supported |
| Tailnet TCP listening/dialing | `Listen("tcp", ...)`, `Dial(ctx, "tcp", ...)` | `tcp_listen(SocketAddr)`, `tcp_connect(SocketAddr)` | Supported primitive; adapt listener semantics and deadlines |
| gRPC, streaming, redials, keepalive | Custom gRPC dialer over tsnet | Streams implement Tokio asynchronous I/O | Plausible Rust gRPC integration; no existing Go replacement or proven Herdr parity |
| Coordinator discovery by assigned DNS name | Full/short MagicDNS targets | `peer_by_name()` indexes hostname and FQDN | Adapter can resolve a visible peer into an IP; full DNS resolver remains unsupported |
| Connected peer stable ID, name, assigned tags | `LocalClient().WhoIs(remoteAddress)` | `peer_by_tailnet_ip()`, `NodeInfo` fields | Adapter can cover ordinary direct tailnet peers |
| Fresh role revocation / removed peer handling | Repeated WhoIs at admission and dispatch | Peer tracker applies full/delta updates and deletions | Mechanism exists; verify role changes and redials end to end |
| Local coordinator and worker share identity | Dial own assigned name/IP; self WhoIs | Known loopback issue; peer tracker excludes separate self node | Important unresolved managed-mode requirement |
| Complete self/status diagnostics | `Up()` returns `ipnstate.Status` | `self_node()` exposes a subset | Gap: MagicDNS enablement, tailnet display identity, health/backend status, and other status fields |
| Read-only HTTP dashboard on tailnet | tsnet TCP listener + peer lookup | TCP plus optional axum listener adapter | Feasible adapter; retain peer checks and exact-origin behavior |
| Optional HTTPS dashboard | `ListenTLS()` / Tailscale certificate provisioning | HTTPS certificates listed unsupported | Gap; Rust TLS alone does not issue or renew the node's Tailscale certificate |
| Stop/start and graceful shutdown | `Close()`, preserve enrollment | `shutdown(timeout)` | Supported primitive; integrate Herdr role shutdown and ownership |
| Existing backup and recovery state | Requires `tsnet/tailscaled.state` | Different serialized key-file format | Application changes and migration strategy required |
| Existing release architectures | Windows/Linux/macOS, amd64/arm64 | Advertised Linux x86_64/ARM64, macOS ARM64, Windows x86_64 | Windows ARM64 and macOS x86_64 are outside advertised/CI coverage |
| Control-plane network policy | Tailscale grants/ACLs plus application role checks | Runtime applies packet-filter updates and authenticated source-IP filtering | Implemented mechanism; verify this project's grants and revocation behavior |

Rust API evidence: [Device](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/src/lib.rs), [Config](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/src/config.rs), [NodeInfo](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/ts_control/src/node.rs), [peer database](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/ts_runtime/src/peer_tracker/peer_db.rs), [packet filtering](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/ts_runtime/src/packetfilter.rs), [source filtering](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/ts_runtime/src/src_filter.rs).

## Important findings

### Peer identity can be adapted without a literal WhoIs API

Herdr's `PeerIdentity` needs only stable ID, name, and assigned tags. Rust's `NodeInfo` contains these, and accepted/dialed TCP streams expose the actual remote address. An adapter can parse that address, query `peer_by_tailnet_ip(remote.ip())`, and construct Herdr's identity. It must reject unknown/missing identities and check exact assigned tags every time the existing code would call WhoIs.

The lookup must use the connected stream's remote IP, not the configured hostname, request metadata, or requested tags. Looking up the hostname before connecting is useful for routing, but does not replace authenticating the connected endpoint. Rust's incoming source filter ties permitted source IPs to the encrypted peer; that mechanism is important to the validity of IP-based identification.

The [peer tracker](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/ts_runtime/src/peer_tracker/mod.rs) handles full replacement, changed peers, patches, and removals. Assigned tag changes can arrive through complete changed-node records. A fresh lookup sees the actor's current control-plane view, analogous in purpose to local WhoIs; it is not a synchronous query to the remote control server. Herdr should continue checking stable identity and tags at command dispatch, on incoming RPCs, and on every underlying reconnect. Source support is not evidence that all race/revocation cases already pass.

### Shared-identity managed mode is the most important connectivity risk

`src/internal/meshlocal/runtime.go` creates one enrolled network per computer. On the coordinator, the server, local worker, and local client path share it; the target becomes the coordinator's own assigned full DNS name. Herdr also has a real Go control-plane test proving self WhoIs observes role revocation.

Upstream [issue #176](https://github.com/tailscale/tailscale-rs/issues/176), “dataplane: loopback to self-node not working,” was open when checked. Its concrete reproduction is UDP to the device's own tailnet address. The inspected route updater builds outbound routes from peers and self-node inbound routes separately; it does not install an outbound self-loopback route. The peer tracker likewise indexes peer updates, not the distinct self node.

This is strong evidence of a managed-mode gap, but the issue's UDP reproduction is not a live proof that Herdr's exact TCP/gRPC path fails. The [basic Rust tests](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/tests/basic.rs) include TCP self-connections, yet they return without exercising the network unless `TS_RS_TEST_NET` is enabled; see [test gating](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/ts_test_util/src/lib.rs). Their presence does not close the issue.

Before migrating managed mode, prove self dialing, accepted remote addresses, and self identity checks on a real device. Self identification needs a fresh `self_node()` branch for addresses matching this device. Preserve assigned-tag revocation checks; do not introduce an unconditional local authorization bypass. Fixing routing may require upstream changes. Separate identities or host-loopback shortcuts would alter the current product architecture.

### Name lookup is available; managed DNS status is missing

The README still describes peer hostname lookup as forthcoming, but actual Device and peer-database code expose it. The database stores both bare hostname and assigned FQDN and trims trailing dots at lookup. Use full assigned names to avoid short-name collisions and define casing behavior explicitly.

That is enough for Herdr to implement ordinary coordinator-name resolution without a full DNS server. It does not provide DNS forwarding, split DNS, or all MagicDNS functionality.

Managed startup currently refuses to proceed unless `SelfStatus.MagicDNSEnabled` is true and validates the configured tailnet against both the reported tailnet name and DNS suffix. Rust `NodeInfo.tailnet` is parsed from the node FQDN: it represents the DNS suffix, not necessarily the Go `CurrentTailnet.Name` value. Rust parses DNS configuration and the control domain at the serialization layer, but its [StateUpdate](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/ts_control/src/client/map_stream.rs) does not retain/expose them to Device callers.

Therefore, deriving an FQDN and setting `magic_dns_enabled = true` would lose a current managed-startup guarantee. Preserve actual control-plane DNS enablement and tailnet identity through an upstream API extension or an explicitly reviewed contract change. Backend state and health diagnostics also lack the equivalent public API. Assigned tags, IPs, stable ID, DNS name, and key-expiry time can be mapped today; a complete `SelfStatus` cannot.

### HTTPS certificates and state continuity are independent gaps

The HTTP dashboard is feasible over the existing Rust TCP listener. The optional HTTPS path currently calls Go `ListenTLS`, which obtains Tailscale-assisted certificates. Upstream [status](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/README.md#status) lists HTTPS certificates as unsupported. Wrapping a Rust stream in TLS supplies encryption only after a certificate is provided; issuance, renewal, authorized names, and failure handling still need an implementation. Default managed HTTP does not require browser TLS, but the user asked about all currently used features, so the optional path counts.

Persistent Rust identities are supported, with non-ephemeral enrollment now the default. Rust's [key-file helper](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/src/config.rs) reads/writes its own JSON format; it does not import Go `tailscaled.state`. It also does not itself provide exclusive file ownership or the private/atomic persistence behavior Herdr must retain. Application ownership guards already exist and should be preserved.

Re-enrollment can produce a new Tailscale stable identity. That matters because Herdr persists stable-ID bindings, actor identities, and managed remote-deletion identity. It also explicitly requires the Go state file in offline role backups. Changing the library requires a reviewed identity-continuity/re-enrollment path and updated backup validation; keeping the same directory name is insufficient. No Go-to-Rust state importer was established in this review.

### Current maturity is better than the initial preview

The April preview's DERP-only and unaudited-cryptography descriptions are outdated. The pinned [September changelog](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/CHANGELOG.md#060---2026-09-17) reports a completed third-party audit of `ts_tunnel` cryptography and remediation of security-relevant findings. It removes the `TS_RS_EXPERIMENT` requirement. This is a specific audit statement, not certification of every component or Herdr's integration.

Direct connections now work in many cases, with DERP fallback and incomplete NAT traversal. The current README and [CI matrix](https://github.com/tailscale/tailscale-rs/blob/ede063fa7e1f21d36e658170c2e0d5347b944e6b/.github/workflows/ci.yml) include Windows x86_64 and macOS ARM64. Some crate-level introductory comments still describe the earlier preview, so release notes and implementation were used to resolve those inconsistencies. APIs remain pre-1.0, and main has recently consolidated/refactored internal crates. Prefer public APIs and a pinned release for a spike.

## Migration implications and acceptance gates

This repository is Go. Rust `Device` is not a replacement for `*tsnet.Server`, and Rust streams are not Go `net.Conn`. A full Rust port would need gRPC stream/listener adapters and the existing authorization, cancellation, message-limit, keepalive, reconnect, dashboard, and lifecycle semantics. TCP/Tokio support makes that plausible; it does not establish application parity.

Retaining the Go application with Rust networking would instead require a maintained FFI boundary and a Go `net.Conn`/listener adapter. The inspected C exports provide sockets and peer address lookup, but not the complete Rust NodeInfo/auth-state interface Herdr needs for stable IDs, assigned tags, and browser enrollment. Additional exports would be needed.

A useful experimental acceptance sequence is:

1. Create a fresh non-ephemeral Rust identity through browser enrollment; verify assigned roles, cancellation, approval failures, restart, and unchanged stable ID.
2. Prove same-device TCP/gRPC connections and fresh self-role revocation with server, worker, and local client sharing one identity. Treat this as an early go/no-go gate.
3. Resolve full coordinator names and connect Rust-to-Go and Go-to-Rust over IPv4/IPv6; authorize using actual remote addresses and reject wrong tags, removed peers, and role changes before dispatch and after redial.
4. Exercise the project's tailnet grants, dashboard access, long-lived gRPC streams, bounded snapshots, cancellation, sleep/network changes, DERP fallback, and shutdown under load.
5. Restore the full managed status contract and HTTPS issuance/renewal behavior, then validate every advertised release architecture.
6. Design and test identity migration, stable-ID bindings, stopped-state backup/verification, and exact-device teardown before changing existing installations.

**Recommendation:** retain Go tsnet for the existing release. A focused Rust transport experiment is justified, particularly for a future native Rust integration, but full parity requires upstream/API work and application migration work. No migration decision or implementation was made by this assessment.

## Follow-up: Rust 3D mesh visualization

The user clarified that the Rust requirement is for a 3D visualization of current mesh status using Rust and gpurs. This narrows the problem substantially: the renderer needs fleet observations, while the existing backend can continue owning Tailscale enrollment and authorization. The full-backend parity assessment above should not be treated as the minimum requirement for a standalone viewer.

### Existing local IPC is the smallest integration

`Fleet.WatchNodes` already streams complete `NodeList` replacement snapshots. `NodeView` includes node identity, connectivity, staleness, session and Herdr inventory, agent readiness, and observation timestamps. The managed daemon forwards Fleet RPCs over private Unix-domain sockets on Linux/macOS and named pipes on Windows. Rust can generate a client from `api/proto/agentflow/v1/control.proto` and use Tonic with a custom local connector. The UI then shares the daemon's enrolled identity without creating another node.

The work is generated protocol types, IPC endpoint discovery, platform transport and owner checks, protocol compatibility, bounded stream renewal/reconnect, and scene-state projection. Preserve the Unix short-path fallback and Windows pipe-server owner verification. The gateway forwards Fleet mutation methods too; a read-only renderer should restrict its own interface to observation calls, and a separately restricted server endpoint would be needed if backend-enforced read-only access is required.

Watch subscriptions require a deadline of no more than five minutes, support at most eight upstream subscriptions, and allow snapshots up to 8 MiB. The watcher checks state roughly once per second and sends changed replacement snapshots. Rust must renew streams, configure its decode limit accordingly, and mark retained data stale during gaps. Maintain the latest owned snapshot outside the render loop and animate from that cache; neither IPC nor FFI needs to run once per frame.

Primary library references: [Tonic custom connectors](https://docs.rs/tonic/latest/tonic/transport/struct.Endpoint.html#method.connect_with_connector) and [Tokio Windows named pipes](https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/index.html).

### In-process Go interop is moderate, bounded engineering

If independent enrollment inside the Rust executable is required, Go can export a small C ABI using `//export` and `go build -buildmode=c-shared` or, where supported by the target, `c-archive`. Rust calls that ABI through a safe wrapper. The Go runtime remains part of the executable/process, and target-specific linking/packaging is additional work. See [Go build modes](https://pkg.go.dev/cmd/go#hdr-Build_modes).

For this use case, wrap a mesh observation client rather than every tsnet socket operation: create/connect a client, start a fleet watch, fetch the next bounded snapshot or gap event, cancel, close, and release an output buffer. Go retains tsnet dialing, actual-peer WhoIs/tag checks, gRPC, retries, and stream limits. Rust receives copied serialized snapshots or a stable data representation. Integer handles and explicitly owned buffers avoid exposing Go objects, slices, interfaces, or Go-owned pointers across the ABI; [cgo pointer rules](https://pkg.go.dev/cmd/cgo#hdr-Passing_pointers) apply.

A blocking next-event call on a worker thread is a simple initial design. Cancellation must wake it, close must join outstanding work, and Rust must release every returned allocation through the matching exported allocator function. Avoid callbacks into the graphics thread. This concentrates the hard work in ownership, lifetime, cancellation, shutdown races, error/version contracts, and native-library packaging, rather than in networking feature development. A live tsnet identity directory cannot be shared concurrently with the existing daemon: use local IPC or distinct enrollment/state for an independently embedded client.

Official [libtailscale](https://github.com/tailscale/libtailscale) already demonstrates Go tsnet behind a C ABI. Its inspected header exposes node lifecycle, dial/listen, status JSON, and loopback LocalAPI access. However, its implementation uses Unix socket pairs and Unix descriptor passing, lacks a requested-tags setter in the examined header, and pins Go Tailscale v1.94.1 rather than this project's v1.102.4. It is a useful reference, not an established cross-platform drop-in for this project. A small wrapper within this repository can use the existing pinned transport and role checks directly. References: [header](https://github.com/tailscale/libtailscale/blob/main/tailscale.h), [implementation](https://github.com/tailscale/libtailscale/blob/main/tailscale.go), [dependency pin](https://github.com/tailscale/libtailscale/blob/main/go.mod).

### Relative effort

Compare required implementation, validation, and unresolved risks. This comparison concerns observation integration; building the 3D renderer is separate.

| Approach | Required work | Main uncertainty | Enrollment impact |
|---|---|---|---|
| Rust consumes existing daemon IPC | Generate Fleet client; implement private IPC discovery/owner checks; renew/reconnect watches; project snapshots into scene state | Cross-platform connectors and recovery behavior | Reuses the daemon's existing identity on the same machine |
| Rust embeds a narrow Go mesh-client ABI | Export observation API; implement safe handles/buffers, cancellation and shutdown; build/package native library | Lifetime races and Go/Rust/C toolchain integration | Independently embedded client needs distinct enrollment/state |
| Rust embeds a general tsnet socket wrapper | Above plus async socket adapters, listeners, status, peer authorization and TLS APIs | Broader API surface and platform socket behavior | Independently embedded client needs distinct enrollment/state |
| Standalone viewer using native Rust Tailscale | Integrate Device with gRPC; implement enrollment/persistence, name lookup, actual-peer role checks and watch recovery | Real interoperability and target-platform behavior | Viewer enrolls as its own client identity |
| Replace the entire Go backend with Rust Tailscale | Resolve upstream feature gaps and migrate backend transport, status, certificates and identity state | Full parity and migration correctness | Requires an explicit identity-continuity plan |

A standalone Rust viewer does not inherently need self dialing, to serve HTTPS, or to import the coordinator's existing Go state. Those earlier gaps mostly cease to matter when the backend stays in Go and the viewer has its own client identity. Native Rust therefore remains a credible viewer experiment. A narrow Go bridge offers the more predictable route to matching existing enrollment, name resolution, authorization and reconnect semantics; it is substantially less work than implementing all missing backend features in the Rust library.

If “mesh status” includes network-path topology, RTT, DERP/direct selection, or traffic rates, those values are not in the current NodeView schema. Add explicit Go telemetry collection and a protocol projection regardless of the viewer's transport choice. A tailnet peer map alone cannot supply Herdr session/agent state or an authoritative global network-path graph.

**Visualization recommendation at assessment time:** use existing local IPC for a renderer running alongside managed Herdr. If the application must enroll independently in one Rust process, start with a narrow Go observation-client ABI. Consider native Rust Tailscale once a dedicated viewer spike verifies its smaller requirement set. The later implemented viewer uses a dedicated read-only loopback gRPC service on the managed daemon, rather than its full-control private IPC endpoint.
