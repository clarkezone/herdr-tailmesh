package meshlocal

import (
	"bytes"
	"context"
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
}

func (f *observerFleet) WatchNodes(_ *emptypb.Empty, s grpc.ServerStreamingServer[pb.NodeList]) error {
	return f.watch(s)
}

func observerFixture(t *testing.T, watch func(grpc.ServerStreamingServer[pb.NodeList]) error) (pb.LocalObserverClient, *grpc.ClientConn, string) {
	t.Helper()
	upListener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	fleet := grpc.NewServer(grpc.MaxSendMsgSize(maxResponse + 1024))
	pb.RegisterFleetServer(fleet, &observerFleet{watch: watch})
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
	if err != nil || !strings.Contains(string(output), "1 agents, 1 working") {
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
		err := s.Send(&pb.NodeList{Nodes: []*pb.NodeView{{InstanceId: "node-1", Hostname: "Development", Connected: true, LastSeen: received, Herdr: herdr, HerdrReceivedAt: received, SessionsReady: true, Sessions: []*pb.SessionView{{Name: "native", Incarnation: "incarnation-1", Status: "ready", Herdr: herdr, HerdrReceivedAt: received}}}, {InstanceId: "node-2", Hostname: "Offline workstation", Connected: false}}})
		if err != nil {
			return err
		}
		<-s.Context().Done()
		return nil
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
