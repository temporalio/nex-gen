package tests

import (
	"context"
	"testing"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/suite"
	workflowservicepb "go.temporal.io/api/workflowservice/v1"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

	ws "examples/go/workflowservice"
)

const workflowServiceName = "temporal.api.workflowservice.v1.WorkflowService"

func signalWithStartWorkflow(ctx workflow.Context, input string) string {
	return input
}

type WorkflowServiceIntegrationSuite struct {
	suite.Suite
	testsuite.WorkflowTestSuite
	env   *testsuite.TestWorkflowEnvironment
	calls []*workflowservicepb.SignalWithStartWorkflowExecutionRequest
}

func (s *WorkflowServiceIntegrationSuite) SetupTest() {
	s.env = s.NewTestWorkflowEnvironment()
	s.calls = nil

	signalWithStart := nexus.NewSyncOperation("SignalWithStartWorkflowExecution",
		func(ctx context.Context, input *workflowservicepb.SignalWithStartWorkflowExecutionRequest, opts nexus.StartOperationOptions) (*workflowservicepb.SignalWithStartWorkflowExecutionResponse, error) {
			s.calls = append(s.calls, input)
			return &workflowservicepb.SignalWithStartWorkflowExecutionResponse{}, nil
		})

	service := nexus.NewService(workflowServiceName)
	s.NoError(service.Register(signalWithStart))
	s.env.RegisterNexusService(service)
}

func TestWorkflowServiceIntegrationSuite(t *testing.T) {
	suite.Run(t, new(WorkflowServiceIntegrationSuite))
}

func (s *WorkflowServiceIntegrationSuite) TestSignalWithStartWorkflowEncodesSingleSignalArg() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*ws.SignalWithStartWorkflowResponse, error) {
		return getFutureResult[ws.SignalWithStartWorkflowResponse](ctx, ws.SignalWithStartWorkflow(
			ctx,
			ws.SignalWithStartWorkflowOptions{
				Id:        "workflow-id",
				TaskQueue: "task-queue",
			},
			"wake-up",
			"signal-value",
			signalWithStartWorkflow,
			"workflow-input",
		))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())

	s.Require().Len(s.calls, 1)
	request := s.calls[0]
	s.Equal("wake-up", request.GetSignalName())
	s.Require().NotNil(request.GetSignalInput())
	s.Len(request.GetSignalInput().GetPayloads(), 1)
	s.Require().NotNil(request.GetInput())
	s.Len(request.GetInput().GetPayloads(), 1)
}

func (s *WorkflowServiceIntegrationSuite) TestSignalWithStartWorkflowWithArgsEncodesNilSignalArg() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*ws.SignalWithStartWorkflowResponse, error) {
		return getFutureResult[ws.SignalWithStartWorkflowResponse](ctx, ws.SignalWithStartWorkflowWithArgs(
			ctx,
			ws.SignalWithStartWorkflowOptions{
				Id:        "workflow-id",
				TaskQueue: "task-queue",
			},
			"wake-up",
			nil,
			"ExampleWorkflow",
			"one",
			"two",
		))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())

	s.Require().Len(s.calls, 1)
	request := s.calls[0]
	s.Equal("wake-up", request.GetSignalName())
	s.Require().NotNil(request.GetSignalInput())
	s.Len(request.GetSignalInput().GetPayloads(), 1)
	s.Require().NotNil(request.GetInput())
	s.Len(request.GetInput().GetPayloads(), 2)
}
