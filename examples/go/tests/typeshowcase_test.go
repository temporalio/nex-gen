package tests

import (
	"context"
	"testing"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/suite"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

	"examples/go/typeshowcase"
)

// --- Integration tests ---

// Exercises the generic Tuple2/Result helper types generated for tuples and
// results inside containers (lists and maps), round-tripping them through an
// in-process Nexus handler.
type TypeShowcaseIntegrationSuite struct {
	suite.Suite
	testsuite.WorkflowTestSuite
	env   *testsuite.TestWorkflowEnvironment
	calls []testCall
}

func (s *TypeShowcaseIntegrationSuite) SetupTest() {
	s.env = s.NewTestWorkflowEnvironment()
	s.calls = nil

	recordSync := nexus.NewSyncOperation(typeshowcase.RecordSyncOp,
		func(ctx context.Context, input typeshowcase.RecordSyncRequest, opts nexus.StartOperationOptions) (nexus.NoValue, error) {
			s.calls = append(s.calls, testCall{"RecordSync", input})
			return nil, nil
		})

	service := nexus.NewService(typeshowcase.ServiceName)
	s.NoError(service.Register(recordSync))
	s.env.RegisterNexusService(service)
}

func TestTypeShowcaseIntegrationSuite(t *testing.T) {
	suite.Run(t, new(TypeShowcaseIntegrationSuite))
}

func sampleSyncReport() typeshowcase.SyncReport {
	return typeshowcase.SyncReport{
		Route: []typeshowcase.Tuple2[float64, float64]{
			{First: 45.5152, Second: -122.6784},
			{First: 47.6062, Second: -122.3321},
		},
		Attempts: []typeshowcase.Result[string, string]{
			{Result: "synced"},
			{Error: "timeout"},
		},
		RegionStatus: map[string]typeshowcase.Result[string, string]{
			"west":    {Result: "healthy"},
			"central": {Error: "degraded"},
		},
	}
}

func (s *TypeShowcaseIntegrationSuite) TestRecordSync() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) error {
		return typeshowcase.RecordSync(ctx, "user-123", sampleSyncReport())
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())

	s.Require().Len(s.calls, 1)
	s.Equal("RecordSync", s.calls[0].Operation)
	s.Equal(typeshowcase.RecordSyncRequest{
		UserId: "user-123",
		Report: sampleSyncReport(),
	}, s.calls[0].Input)
}
