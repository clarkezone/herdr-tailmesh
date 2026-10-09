package meshlocal

import (
	"bytes"
	"context"
	"fmt"
	"net"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"

	pb "github.com/clarkezone/herdr-distributed-mesh/src/gen/agentflow/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/emptypb"
	"google.golang.org/protobuf/types/known/timestamppb"
)

type observerFleet struct {
	pb.UnimplementedFleetServer
	watch func(grpc.ServerStreamingServer[pb.NodeList]) error
	info  func(context.Context) (*pb.ServerInfo, error)
}

func (f *observerFleet) GetServerInfo(ctx context.Context, _ *emptypb.Empty) (*pb.ServerInfo, error) {
	if f.info != nil {
		return f.info(ctx)
	}
	return &pb.ServerInfo{InstanceId: "upstream-coordinator", ImplementationVersion: "fixture-version"}, nil
}

func (f *observerFleet) WatchNodes(_ *emptypb.Empty, s grpc.ServerStreamingServer[pb.NodeList]) error {
	return f.watch(s)
}

func observerFixture(t *testing.T, watch func(grpc.ServerStreamingServer[pb.NodeList]) error, infos ...func(context.Context) (*pb.ServerInfo, error)) (pb.LocalObserverClient, *grpc.ClientConn, string) {
	t.Helper()
	upListener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	fleet := grpc.NewServer(grpc.MaxSendMsgSize(maxResponse + 1024))
	fixture := &observerFleet{watch: watch}
	if len(infos) > 0 {
		fixture.info = infos[0]
	}
	pb.RegisterFleetServer(fleet, fixture)
	go fleet.Serve(upListener)
	up, err := grpc.NewClient(upListener.Addr().String(), grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	server := newObserver(up, "local-node")
	go server.Serve(listener)
	client, err := grpc.NewClient(listener.Addr().String(), grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		client.Close()
		server.Stop()
		listener.Close()
		up.Close()
		fleet.Stop()
		upListener.Close()
	})
	return pb.NewLocalObserverClient(client), client, listener.Addr().String()
}

func TestObserverOnlyRegistersReadMethodsAndSanitizesErrors(t *testing.T) {
	observer, conn, _ := observerFixture(t, func(s grpc.ServerStreamingServer[pb.NodeList]) error {
		return status.Error(codes.Unavailable, "private upstream diagnostic")
	})
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	info, err := observer.GetInfo(ctx, &emptypb.Empty{})
	if err != nil || info.GetApiVersion() != 1 || info.GetDaemonName() != "local-node" {
		t.Fatalf("info=%v err=%v", info, err)
	}
	if info.GetCoordinator().GetInstanceId() != "upstream-coordinator" || info.GetCoordinator().GetImplementationVersion() != "fixture-version" {
		t.Fatalf("coordinator not relayed: %v", info)
	}
	for _, method := range []string{pb.Fleet_ListNodes_FullMethodName, pb.Fleet_SubmitCommand_FullMethodName, "/grpc.reflection.v1.ServerReflection/ServerReflectionInfo"} {
		err := conn.Invoke(ctx, method, &emptypb.Empty{}, &emptypb.Empty{})
		if status.Code(err) != codes.Unimplemented {
			t.Fatalf("%s exposed: %v", method, err)
		}
	}
	stream, err := observer.WatchNodes(ctx, &emptypb.Empty{})
	if err != nil {
		t.Fatal(err)
	}
	_, err = stream.Recv()
	if status.Code(err) != codes.Unavailable || strings.Contains(err.Error(), "private") {
		t.Fatalf("unsafe error: %v", err)
	}
}

func TestObserverBoundsMetadataCancellationAndLargeSnapshots(t *testing.T) {
	started := make(chan struct{})
	stopped := make(chan struct{})
	observer, _, _ := observerFixture(t, func(s grpc.ServerStreamingServer[pb.NodeList]) error {
		defer close(stopped)
		md, _ := metadata.FromIncomingContext(s.Context())
		if len(md.Get("authorization")) != 0 {
			t.Error("caller credentials forwarded")
		}
		deadline, ok := s.Context().Deadline()
		if !ok || time.Until(deadline) > observerWatchDuration+time.Second {
			t.Error("unbounded upstream watch")
		}
		close(started)
		if err := s.Send(&pb.NodeList{Nodes: []*pb.NodeView{{InstanceId: "node", Hostname: strings.Repeat("x", 5*1024*1024)}}}); err != nil {
			return err
		}
		<-s.Context().Done()
		return s.Context().Err()
	})
	ctx, cancel := context.WithTimeout(metadata.NewOutgoingContext(context.Background(), metadata.Pairs("authorization", "caller-secret")), 10*time.Second)
	defer cancel()
	stream, err := observer.WatchNodes(ctx, &emptypb.Empty{}, grpc.MaxCallRecvMsgSize(maxResponse))
	if err != nil {
		t.Fatal(err)
	}
	snapshot, err := stream.Recv()
	if err != nil || len(snapshot.GetNodes()[0].GetHostname()) != 5*1024*1024 {
		t.Fatalf("large snapshot: %v", err)
	}
	<-started
	cancel()
	select {
	case <-stopped:
	case <-time.After(5 * time.Second):
		t.Fatal("upstream watch leaked after local cancellation")
	}
}

func TestObserverSubscriptionCapacity(t *testing.T) {
	started := make(chan struct{}, 4)
	observer, _, _ := observerFixture(t, func(s grpc.ServerStreamingServer[pb.NodeList]) error {
		started <- struct{}{}
		if err := s.Send(&pb.NodeList{}); err != nil {
			return err
		}
		<-s.Context().Done()
		return nil
	})
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	for i := 0; i < 4; i++ {
		s, err := observer.WatchNodes(ctx, &emptypb.Empty{})
		if err != nil {
			t.Fatal(err)
		}
		if _, err := s.Recv(); err != nil {
			t.Fatal(err)
		}
		<-started
	}
	s, err := observer.WatchNodes(ctx, &emptypb.Empty{})
	if err == nil {
		_, err = s.Recv()
	}
	if status.Code(err) != codes.ResourceExhausted {
		t.Fatalf("fifth watch: %v", err)
	}
}

func TestObserverRejectsOversizedSnapshots(t *testing.T) {
	observer, _, _ := observerFixture(t, func(s grpc.ServerStreamingServer[pb.NodeList]) error {
		return s.Send(&pb.NodeList{Nodes: []*pb.NodeView{{InstanceId: "node", Hostname: strings.Repeat("x", maxResponse)}}})
	})
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	stream, err := observer.WatchNodes(ctx, &emptypb.Empty{}, grpc.MaxCallRecvMsgSize(maxResponse))
	if err == nil {
		_, err = stream.Recv()
	}
	if status.Code(err) != codes.ResourceExhausted {
		t.Fatalf("oversized snapshot: %v", err)
	}
}

func TestObserverBindFailureIsOptional(t *testing.T) {
	l, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer l.Close()
	var warning bytes.Buffer
	stop := startObserver(nil, "node", l.Addr().(*net.TCPAddr).Port, &warning)
	stop()
	if !strings.Contains(warning.String(), "Warning:") {
		t.Fatal("missing nonfatal bind warning")
	}
}

func TestRustObserverWireContract(t *testing.T) {
	binary := os.Getenv("HERDR_VISUALIZER_TEST_BINARY")
	if binary == "" {
		t.Skip("set HERDR_VISUALIZER_TEST_BINARY to the built Rust executable")
	}
	_, _, address := observerFixture(t, func(s grpc.ServerStreamingServer[pb.NodeList]) error {
		return s.Send(&pb.NodeList{Nodes: []*pb.NodeView{{InstanceId: "wire-node", Connected: true, Hostname: strings.Repeat("x", 5*1024*1024), Herdr: &pb.HerdrState{Status: "ready", Agents: []*pb.HerdrEntity{{Id: "agent", AgentStatus: "working"}}}}}})
	})
	_, port, _ := net.SplitHostPort(address)
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	output, err := exec.CommandContext(ctx, binary, "--check", "--port", port).CombinedOutput()
	if err != nil || !strings.Contains(string(output), "1 agents, 1 working") || !strings.Contains(string(output), "coordinator: upstream-coordinator") {
		t.Fatalf("Rust wire check: %v %s", err, output)
	}
}

// Opt-in native GPU smoke, using the real Go observer bridge without enrollment.
func TestRustObserverWindowSmoke(t *testing.T) {
	if os.Getenv("HERDR_VISUALIZER_WINDOW_SMOKE") != "1" {
		t.Skip("opt-in graphical smoke")
	}
	binary := os.Getenv("HERDR_VISUALIZER_TEST_BINARY")
	if binary == "" {
		t.Fatal("HERDR_VISUALIZER_TEST_BINARY is required")
	}
	received := timestamppb.Now()
	herdr := &pb.HerdrState{Status: "ready", Version: "fixture", Workspaces: []*pb.HerdrEntity{{Id: "workspace-1", DisplayName: "Mesh renderer", ProjectId: "herdr-mesh"}}, Agents: []*pb.HerdrEntity{{Id: "agent-1", WorkspaceId: "workspace-1", DisplayName: "Visualizer", AgentStatus: "working", Provider: "codex"}, {Id: "agent-2", WorkspaceId: "workspace-1", DisplayName: "Review", AgentStatus: "blocked", Provider: "codex"}}}
	_, _, address := observerFixture(t, func(s grpc.ServerStreamingServer[pb.NodeList]) error {
		snapshot := &pb.NodeList{Nodes: []*pb.NodeView{{InstanceId: "node-1", Hostname: "Development", Connected: true, LastSeen: received, Herdr: herdr, HerdrReceivedAt: received, SessionsReady: true, Sessions: []*pb.SessionView{{Name: "native", Incarnation: "incarnation-1", Status: "ready", Herdr: herdr, HerdrReceivedAt: received}}}, {InstanceId: "node-2", Hostname: "Offline workstation", Connected: false}}}
		ticker := time.NewTicker(2 * time.Second)
		defer ticker.Stop()
		for {
			if err := s.Send(snapshot); err != nil {
				return err
			}
			select {
			case <-s.Context().Done():
				return nil
			case <-ticker.C:
				stamp := timestamppb.Now()
				snapshot.Nodes[0].LastSeen = stamp
				snapshot.Nodes[0].HerdrReceivedAt = stamp
				snapshot.Nodes[0].Sessions[0].HerdrReceivedAt = stamp
			}
		}
	})
	_, port, _ := net.SplitHostPort(address)
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()
	cmd := exec.CommandContext(ctx, binary, "--port", port)
	var output bytes.Buffer
	cmd.Stdout, cmd.Stderr = &output, &output
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	t.Log("Native window smoke running; close the viewer after inspecting it")
	err := cmd.Wait()
	if ctx.Err() == nil && err != nil {
		t.Fatalf("native viewer failed: %v %s", err, &output)
	}
	if ctx.Err() != nil {
		t.Log("Smoke window stopped at the inspection deadline")
	}
}

func TestObserverInfoIsolationAndUnavailableFallback(t *testing.T) {
	cases := []struct {
		name  string
		info  *pb.ServerInfo
		err   error
		known bool
	}{
		{name: "verified", info: &pb.ServerInfo{InstanceId: "actual-upstream", ImplementationVersion: "1.2", Capabilities: []string{"private"}}, known: true},
		{name: "unavailable", err: status.Error(codes.Unavailable, "private diagnostic")},
		{name: "missing", info: &pb.ServerInfo{}},
		{name: "oversized", info: &pb.ServerInfo{InstanceId: strings.Repeat("x", 129)}},
		{name: "control", info: &pb.ServerInfo{InstanceId: "bad\nidentity"}},
		{name: "response-limit", info: &pb.ServerInfo{InstanceId: "actual", ImplementationVersion: strings.Repeat("x", 70*1024)}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			observer, _, _ := observerFixture(t, nil, func(ctx context.Context) (*pb.ServerInfo, error) {
				md, _ := metadata.FromIncomingContext(ctx)
				if len(md.Get("authorization")) != 0 {
					t.Error("info forwarded caller metadata")
				}
				deadline, ok := ctx.Deadline()
				if !ok || time.Until(deadline) > time.Second+100*time.Millisecond {
					t.Error("info request not bounded")
				}
				return tc.info, tc.err
			})
			ctx, cancel := context.WithTimeout(metadata.NewOutgoingContext(context.Background(), metadata.Pairs("authorization", "synthetic-caller")), 3*time.Second)
			defer cancel()
			info, err := observer.GetInfo(ctx, &emptypb.Empty{})
			if err != nil || info.GetDaemonName() != "local-node" || (info.GetCoordinator() != nil) != tc.known {
				t.Fatalf("info=%v err=%v", info, err)
			}
			if tc.known && len(info.GetCoordinator().GetCapabilities()) != 0 {
				t.Fatal("arbitrary properties leaked")
			}
		})
	}
}

func TestObserverInfoDeadlineAndCapacity(t *testing.T) {
	started := make(chan struct{}, 4)
	canceled := make(chan struct{}, 4)
	observer, _, _ := observerFixture(t, nil, func(ctx context.Context) (*pb.ServerInfo, error) {
		started <- struct{}{}
		<-ctx.Done()
		canceled <- struct{}{}
		return nil, ctx.Err()
	})
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	results := make(chan error, 4)
	for i := 0; i < 4; i++ {
		go func() {
			info, err := observer.GetInfo(ctx, &emptypb.Empty{})
			if err == nil && info.GetCoordinator() != nil {
				err = fmt.Errorf("unexpected coordinator")
			}
			results <- err
		}()
	}
	for i := 0; i < 4; i++ {
		select {
		case <-started:
		case <-ctx.Done():
			t.Fatal("info calls not started")
		}
	}
	_, err := observer.GetInfo(ctx, &emptypb.Empty{})
	if status.Code(err) != codes.ResourceExhausted {
		t.Fatalf("fifth info call: %v", err)
	}
	for i := 0; i < 4; i++ {
		select {
		case err := <-results:
			if err != nil {
				t.Fatal(err)
			}
		case <-ctx.Done():
			t.Fatal("deadline did not return local info")
		}
	}
	for i := 0; i < 4; i++ {
		select {
		case <-canceled:
		case <-ctx.Done():
			t.Fatal("upstream request leaked")
		}
	}
}

func TestObserverForwardsMemberImplementationVersionAndLegacyUnknown(t *testing.T) {
	observer, _, _ := observerFixture(t, func(s grpc.ServerStreamingServer[pb.NodeList]) error {
		return s.Send(&pb.NodeList{Nodes: []*pb.NodeView{{InstanceId: "updated", ImplementationVersion: "member-v4"}, {InstanceId: "legacy"}}})
	})
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	stream, err := observer.WatchNodes(ctx, &emptypb.Empty{})
	if err != nil {
		t.Fatal(err)
	}
	nodes, err := stream.Recv()
	if err != nil || len(nodes.GetNodes()) != 2 {
		t.Fatalf("snapshot %v %v", nodes, err)
	}
	if nodes.Nodes[0].ImplementationVersion != "member-v4" || nodes.Nodes[1].ImplementationVersion != "" {
		t.Fatal("member metadata lost or inferred from coordinator")
	}
}
