package onboard

import (
	"context"
	"io"
	"strings"
	"testing"

	"github.com/clarkezone/herdr-distributed-mesh/src/internal/meshlocal"
)

func TestVisualizerOverrideIsPerLaunchAndReplacesInheritedEnvironment(t *testing.T) {
	f := newFixture(t)
	f.d.VisualizerPort = 8791
	f.d.Environment = func() []string {
		return []string{"PATH=provider", meshlocal.VisualizerPortEnv + "=1234", strings.ToLower(meshlocal.VisualizerPortEnv) + "=1235"}
	}
	f.d.Start = func(_, _ string, env []string) error {
		f.starts++
		if strings.Join(env, "|") != "PATH=provider|"+meshlocal.VisualizerPortEnv+"=8791" {
			t.Fatalf("environment: %v", env)
		}
		return nil
	}
	if err := Run(context.Background(), joinOptions(), io.Discard, f.d); err != nil {
		t.Fatal(err)
	}
	if err := Start(context.Background(), io.Discard, f.d); err == nil || !strings.Contains(err.Error(), "shutdown") {
		t.Fatalf("running override = %v", err)
	}
	if f.starts != 1 || f.saved != 1 {
		t.Fatalf("side effects starts=%d saves=%d", f.starts, f.saved)
	}
}

func TestVisualizerPortRejectedBeforeIdentityCreation(t *testing.T) {
	for _, port := range []int{-1, 65536} {
		f := newFixture(t)
		f.d.VisualizerPort = port
		if err := Run(context.Background(), initOptions(), io.Discard, f.d); err == nil {
			t.Fatal("invalid port accepted")
		}
		if f.saved != 0 || f.policyCalls != 0 || f.starts != 0 {
			t.Fatal("invalid override had effects")
		}
	}
}
