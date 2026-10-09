package server

import (
	"unicode"
	"unicode/utf8"

	pb "github.com/clarkezone/herdr-distributed-mesh/src/gen/agentflow/v1"
	"github.com/clarkezone/herdr-distributed-mesh/src/internal/transport"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/proto"
)

func validateLogicalNodeName(name string) error {
	if name != "" && !transport.ValidNodeName(name) {
		return status.Error(codes.InvalidArgument, "invalid logical node name")
	}
	return nil
}

// Invalid display metadata is unknown; it must not prevent an otherwise valid node connecting.
func displayVersion(version string) string {
	if len(version) > 256 || !utf8.ValidString(version) {
		return ""
	}
	for _, r := range version {
		if unicode.IsControl(r) {
			return ""
		}
	}
	return version
}

func (f *fleetStore) setHelloMetadata(entry *fleetEntry, name, version string) error {
	version = displayVersion(version)
	if err := validateLogicalNodeName(name); err != nil {
		return err
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.storageErr != nil {
		return storageUnavailable()
	}
	if !f.current(entry) {
		return status.Error(codes.Aborted, "node stream superseded")
	}
	if entry.view.Hostname == name && entry.view.ImplementationVersion == version {
		return nil
	}
	view := proto.Clone(entry.view).(*pb.NodeView)
	view.Hostname = name
	view.ImplementationVersion = version
	if err := f.save(view); err != nil {
		return err
	}
	entry.view = view
	return nil
}
