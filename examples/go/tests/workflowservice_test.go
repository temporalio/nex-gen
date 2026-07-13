package tests

import (
	"context"
	"testing"
	"time"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/suite"
	enums "go.temporal.io/api/enums/v1"
	workflowservicepb "go.temporal.io/api/workflowservice/v1"
	"go.temporal.io/sdk/temporal"
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
		ctx = ws.WithWorkflowContextOptions(ctx, ws.WorkflowContextOptions{
			ID:        "workflow-id",
			TaskQueue: "task-queue",
		})
		return getFutureResult[ws.SignalWithStartWorkflowResponse](ctx, ws.SignalWithStartWorkflow(
			ctx,
			ws.SignalWithStartWorkflowOptions{},
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
		ctx = ws.WithWorkflowContextOptions(ctx, ws.WorkflowContextOptions{
			ID:        "workflow-id",
			TaskQueue: "task-queue",
		})
		return getFutureResult[ws.SignalWithStartWorkflowResponse](ctx, ws.SignalWithStartWorkflowWithArgs(
			ctx,
			ws.SignalWithStartWorkflowOptions{},
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

func (s *WorkflowServiceIntegrationSuite) TestWorkflowContextOptionsAreReadFromContext() {
	retryPolicy := &temporal.RetryPolicy{MaximumAttempts: 3}
	searchKey := temporal.NewSearchAttributeKeyKeyword("CustomKeyword")
	searchAttributes := temporal.NewSearchAttributes(searchKey.ValueSet("search-value"))

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*ws.SignalWithStartWorkflowResponse, error) {
		ctx = ws.WithWorkflowContextOptions(ctx, ws.WorkflowContextOptions{
			Namespace:                "target-namespace",
			ID:                       "workflow-id",
			TaskQueue:                "task-queue",
			WorkflowExecutionTimeout: 3 * time.Hour,
			WorkflowRunTimeout:       2 * time.Hour,
			WorkflowTaskTimeout:      time.Minute,
			WorkflowIDReusePolicy:    enums.WORKFLOW_ID_REUSE_POLICY_REJECT_DUPLICATE,
			RetryPolicy:              retryPolicy,
			CronSchedule:             "0 * * * *",
			Memo:                     map[string]any{"memo-key": "memo-value"},
			SearchAttributes:         searchAttributes,
			Priority:                 temporal.Priority{PriorityKey: 7},
		})
		return getFutureResult[ws.SignalWithStartWorkflowResponse](ctx, ws.SignalWithStartWorkflow(
			ctx, ws.SignalWithStartWorkflowOptions{}, "wake-up", "signal-value",
			signalWithStartWorkflow, "workflow-input",
		))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	s.Require().Len(s.calls, 1)
	request := s.calls[0]
	s.Equal("target-namespace", request.GetNamespace())
	s.Equal("workflow-id", request.GetWorkflowId())
	s.Equal("task-queue", request.GetTaskQueue().GetName())
	s.Equal(3*time.Hour, request.GetWorkflowExecutionTimeout().AsDuration())
	s.Equal(2*time.Hour, request.GetWorkflowRunTimeout().AsDuration())
	s.Equal(time.Minute, request.GetWorkflowTaskTimeout().AsDuration())
	s.Equal(enums.WORKFLOW_ID_REUSE_POLICY_REJECT_DUPLICATE, request.GetWorkflowIdReusePolicy())
	s.Equal(int32(3), request.GetRetryPolicy().GetMaximumAttempts())
	s.Equal("0 * * * *", request.GetCronSchedule())
	s.Contains(request.GetMemo().GetFields(), "memo-key")
	s.Contains(request.GetSearchAttributes().GetIndexedFields(), "CustomKeyword")
	s.Equal(int32(7), request.GetPriority().GetPriorityKey())
}

func (s *WorkflowServiceIntegrationSuite) TestWorkflowContextOptionsRequireWorkflowID() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) error {
		ctx = ws.WithWorkflowContextOptions(ctx, ws.WorkflowContextOptions{TaskQueue: "task-queue"})
		ws.SignalWithStartWorkflow(ctx, ws.SignalWithStartWorkflowOptions{}, "wake-up", "signal-value", signalWithStartWorkflow, "workflow-input")
		return nil
	})

	s.ErrorContains(s.env.GetWorkflowError(), "workflow ID is required in WorkflowContextOptions")
	s.Empty(s.calls)
}

func (s *WorkflowServiceIntegrationSuite) TestWorkflowContextOptionsRequireTaskQueue() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) error {
		ctx = ws.WithWorkflowContextOptions(ctx, ws.WorkflowContextOptions{ID: "workflow-id"})
		ws.SignalWithStartWorkflow(ctx, ws.SignalWithStartWorkflowOptions{}, "wake-up", "signal-value", signalWithStartWorkflow, "workflow-input")
		return nil
	})

	s.ErrorContains(s.env.GetWorkflowError(), "task queue is required in WorkflowContextOptions")
	s.Empty(s.calls)
}

func (s *WorkflowServiceIntegrationSuite) TestCanceledContextDoesNotScheduleOperation() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) error {
		ctx = ws.WithWorkflowContextOptions(ctx, ws.WorkflowContextOptions{ID: "workflow-id", TaskQueue: "task-queue"})
		ctx, cancel := workflow.WithCancel(ctx)
		cancel()
		return ws.SignalWithStartWorkflow(ctx, ws.SignalWithStartWorkflowOptions{}, "wake-up", "signal-value", signalWithStartWorkflow, "workflow-input").Get(ctx, nil)
	})

	s.Error(s.env.GetWorkflowError())
	s.Empty(s.calls)
}
