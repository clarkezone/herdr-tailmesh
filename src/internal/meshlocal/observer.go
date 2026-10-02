package meshlocal

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"strconv"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	pb "github.com/clarkezone/herdr-distributed-mesh/src/gen/agentflow/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/emptypb"
)

const DefaultVisualizerPort = 8790
const VisualizerPortEnv = "HERDR_MESH_INTERNAL_VISUALIZER_PORT"
const observerWatchDuration = 4 * time.Minute

func ValidateVisualizerPort(port int) error {
	if port < 1 || port > 65535 {
		return errors.New("--visualizer-port must be between 1 and 65535")
	}
	return nil
}

func visualizerPort() (int, error) {
	value := os.Getenv(VisualizerPortEnv)
	if value == "" {
		return DefaultVisualizerPort, nil
	}
	port, err := strconv.Atoi(value)
	if err != nil {
		return 0, errors.New("invalid internal visualizer port")
	}
	return port, ValidateVisualizerPort(port)
}

type observer struct {
	pb.UnimplementedLocalObserverServer
	name    string
	fleet   pb.FleetClient
	watches chan struct{}
	infos   chan struct{}
}

func newObserver(upstream grpc.ClientConnInterface, name string) *grpc.Server {
	server := grpc.NewServer(grpc.MaxRecvMsgSize(1024), grpc.MaxSendMsgSize(maxResponse), grpc.MaxConcurrentStreams(16))
	pb.RegisterLocalObserverServer(server, &observer{name: name, fleet: pb.NewFleetClient(upstream), watches: make(chan struct{}, 4), infos: make(chan struct{}, 4)})
	return server
}

func (o *observer) GetInfo(ctx context.Context, _ *emptypb.Empty) (*pb.ObserverInfo, error) {
	info := &pb.ObserverInfo{ApiVersion: 1, DaemonName: o.name}
	if !acquire(o.infos) {
		return nil, status.Error(codes.ResourceExhausted, "local observer info capacity reached")
	}
	defer release(o.infos)
	ctx, cancel := context.WithTimeout(metadata.NewOutgoingContext(ctx, metadata.MD{}), time.Second)
	defer cancel()
	remote, err := o.fleet.GetServerInfo(ctx, &emptypb.Empty{}, grpc.MaxCallRecvMsgSize(64*1024))
	if err == nil && remote.GetInstanceId() != "" && len(remote.GetInstanceId()) <= 128 && utf8.ValidString(remote.GetInstanceId()) && strings.IndexFunc(remote.GetInstanceId(), unicode.IsControl) < 0 {
		// Only copy observation properties; never forward arbitrary capabilities
		// or diagnostics. The local endpoint still identifies itself when offline.
		info.Coordinator = &pb.ServerInfo{InstanceId: remote.GetInstanceId()}
		if len(remote.GetImplementationVersion()) <= 256 {
			info.Coordinator.ImplementationVersion = remote.GetImplementationVersion()
		}
	}
	return info, nil
}

func (o *observer) WatchNodes(_ *emptypb.Empty, stream grpc.ServerStreamingServer[pb.NodeList]) error {
	if !acquire(o.watches) {
		return status.Error(codes.ResourceExhausted, "local observer watch capacity reached")
	}
	defer release(o.watches)
	ctx, cancel := context.WithTimeout(metadata.NewOutgoingContext(stream.Context(), metadata.MD{}), observerWatchDuration)
	defer cancel()
	remote, err := o.fleet.WatchNodes(ctx, &emptypb.Empty{}, grpc.MaxCallRecvMsgSize(maxResponse))
	if err != nil {
		return observerError(err)
	}
	for {
		snapshot, err := remote.Recv()
		if errors.Is(err, io.EOF) {
			return nil
		}
		if err != nil {
			return observerError(err)
		}
		if err := stream.Send(snapshot); err != nil {
			return observerError(err)
		}
	}
}

func observerError(err error) error {
	code := status.Code(err)
	if errors.Is(err, context.Canceled) {
		code = codes.Canceled
	}
	if errors.Is(err, context.DeadlineExceeded) {
		code = codes.DeadlineExceeded
	}
	// Never forward upstream addresses, login URLs, metadata or diagnostics.
	return status.Error(code, "mesh observation stream unavailable; reconnect")
}

// Optional UI service has its own lifecycle. Failure never cancels mesh roles.
func startObserver(upstream grpc.ClientConnInterface, name string, port int, output io.Writer) func() {
	listener, err := net.Listen("tcp4", net.JoinHostPort("127.0.0.1", strconv.Itoa(port)))
	if err != nil {
		if output != nil {
			_, _ = fmt.Fprintf(output, "Warning: local visualizer port 127.0.0.1:%d unavailable; restart with --visualizer-port to choose another port.\n", port)
		}
		return func() {}
	}
	server := newObserver(upstream, name)
	done := make(chan struct{})
	go func() {
		defer close(done)
		if err := server.Serve(limitedListener{Listener: listener, gate: make(chan struct{}, 16)}); err != nil && !errors.Is(err, grpc.ErrServerStopped) && output != nil {
			_, _ = fmt.Fprintf(output, "Warning: local visualizer service on 127.0.0.1:%d stopped; mesh remains running.\n", port)
		}
	}()
	return func() { server.Stop(); _ = listener.Close(); <-done }
}
