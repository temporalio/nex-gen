package tests

import (
	"context"
	"testing"
	"time"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/suite"
	activitypb "go.temporal.io/api/activity/v1"
	commonpb "go.temporal.io/api/common/v1"
	taskqueuepb "go.temporal.io/api/taskqueue/v1"
	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"
	"google.golang.org/protobuf/types/known/durationpb"

	tr "examples/go/typeroundtrip"
)

const typeRoundtripServiceName = "TypeRoundtripService"

// --- Mock tests ---

type TypeRoundtripTestSuite struct {
	suite.Suite
	testsuite.WorkflowTestSuite
	env *testsuite.TestWorkflowEnvironment
}

func (s *TypeRoundtripTestSuite) SetupTest() {
	s.env = s.NewTestWorkflowEnvironment()
}

func (s *TypeRoundtripTestSuite) AfterTest(suiteName, testName string) {
	s.env.AssertExpectations(s.T())
}

func TestTypeRoundtripSuite(t *testing.T) {
	suite.Run(t, new(TypeRoundtripTestSuite))
}

func (s *TypeRoundtripTestSuite) TestActivityOptionsOperation() {
	policy := temporal.RetryPolicy{MaximumAttempts: 3}
	priority := temporal.Priority{
		PriorityKey:    4,
		FairnessKey:    "tenant-a",
		FairnessWeight: 2.5,
	}
	protoResult := &activitypb.ActivityOptions{
		TaskQueue:              &taskqueuepb.TaskQueue{Name: "demo-task-queue"},
		RetryPolicy:            &commonpb.RetryPolicy{MaximumAttempts: 3},
		ScheduleToCloseTimeout: durationpb.New(7 * time.Second),
		Priority: &commonpb.Priority{
			PriorityKey:    int32(priority.PriorityKey),
			FairnessKey:    priority.FairnessKey,
			FairnessWeight: priority.FairnessWeight,
		},
	}

	s.env.OnNexusOperation(
		typeRoundtripServiceName,
		nexus.NewOperationReference[*activitypb.ActivityOptions, *activitypb.ActivityOptions]("ActivityOptionsOperation"),
		mock.Anything,
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[*activitypb.ActivityOptions]{
			Value: protoResult,
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*tr.ActivityOptions, error) {
		return getFutureResult[tr.ActivityOptions](
			ctx,
			tr.ActivityOptionsOperation(ctx, policy, tr.ActivityOptionsOperationOptions{
				TaskQueue:              ptr("demo-task-queue"),
				ScheduleToCloseTimeout: ptr(7 * time.Second),
				Priority:               &priority,
			}),
		)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result tr.ActivityOptions
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal(int32(3), result.RetryPolicy.MaximumAttempts)
	s.Require().NotNil(result.TaskQueue)
	s.Equal("demo-task-queue", *result.TaskQueue)
	s.Require().NotNil(result.ScheduleToCloseTimeout)
	s.Equal(7*time.Second, *result.ScheduleToCloseTimeout)
	s.Require().NotNil(result.Priority)
	s.Equal(4, result.Priority.PriorityKey)
	s.Equal("tenant-a", result.Priority.FairnessKey)
	s.InDelta(2.5, float64(result.Priority.FairnessWeight), 0.01)
}

// --- Integration tests ---

type TypeRoundtripIntegrationSuite struct {
	suite.Suite
	testsuite.WorkflowTestSuite
	env   *testsuite.TestWorkflowEnvironment
	calls []testCall
}

func (s *TypeRoundtripIntegrationSuite) SetupTest() {
	s.env = s.NewTestWorkflowEnvironment()
	s.calls = nil

	activityOptionsOp := nexus.NewSyncOperation("ActivityOptionsOperation",
		func(ctx context.Context, input *activitypb.ActivityOptions, opts nexus.StartOperationOptions) (*activitypb.ActivityOptions, error) {
			s.calls = append(s.calls, testCall{"ActivityOptionsOperation", input})
			return input, nil
		})

	service := nexus.NewService(typeRoundtripServiceName)
	s.NoError(service.Register(activityOptionsOp))
	s.env.RegisterNexusService(service)
}

func TestTypeRoundtripIntegrationSuite(t *testing.T) {
	suite.Run(t, new(TypeRoundtripIntegrationSuite))
}

func (s *TypeRoundtripIntegrationSuite) TestActivityOptionsOperation() {
	policy := temporal.RetryPolicy{MaximumAttempts: 3}
	priority := temporal.Priority{
		PriorityKey:    4,
		FairnessKey:    "tenant-a",
		FairnessWeight: 2.5,
	}

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*tr.ActivityOptions, error) {
		return getFutureResult[tr.ActivityOptions](
			ctx,
			tr.ActivityOptionsOperation(ctx, policy, tr.ActivityOptionsOperationOptions{
				TaskQueue:              ptr("demo-task-queue"),
				ScheduleToCloseTimeout: ptr(7 * time.Second),
				Priority:               &priority,
			}),
		)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result tr.ActivityOptions
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal(int32(3), result.RetryPolicy.MaximumAttempts)
	s.Require().NotNil(result.TaskQueue)
	s.Equal("demo-task-queue", *result.TaskQueue)
	s.Require().NotNil(result.ScheduleToCloseTimeout)
	s.Equal(7*time.Second, *result.ScheduleToCloseTimeout)
	s.Require().NotNil(result.Priority)
	s.Equal(4, result.Priority.PriorityKey)
	s.Equal("tenant-a", result.Priority.FairnessKey)
	s.InDelta(2.5, float64(result.Priority.FairnessWeight), 0.01)

	s.Require().Len(s.calls, 1)
	s.Equal("ActivityOptionsOperation", s.calls[0].Operation)
	handlerInput := s.calls[0].Input.(*activitypb.ActivityOptions)
	s.Equal(int32(3), handlerInput.GetRetryPolicy().GetMaximumAttempts())
	s.Equal("demo-task-queue", handlerInput.GetTaskQueue().GetName())
	s.Equal(7*time.Second, handlerInput.GetScheduleToCloseTimeout().AsDuration())
	s.Equal(int32(4), handlerInput.GetPriority().GetPriorityKey())
}

func (s *TypeRoundtripIntegrationSuite) TestActivityOptionsOperationRequiredOnly() {
	policy := temporal.RetryPolicy{MaximumAttempts: 5}

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*tr.ActivityOptions, error) {
		return getFutureResult[tr.ActivityOptions](
			ctx,
			tr.ActivityOptionsOperation(ctx, policy, tr.ActivityOptionsOperationOptions{}),
		)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result tr.ActivityOptions
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal(int32(5), result.RetryPolicy.MaximumAttempts)
	// Optional fields that were never set round-trip back as nil (absent),
	// distinct from a present-but-zero value.
	s.Nil(result.TaskQueue)
	s.Nil(result.ScheduleToCloseTimeout)
	s.Nil(result.Priority)

	s.Require().Len(s.calls, 1)
	s.Equal("ActivityOptionsOperation", s.calls[0].Operation)
	handlerInput := s.calls[0].Input.(*activitypb.ActivityOptions)
	s.Equal(int32(5), handlerInput.GetRetryPolicy().GetMaximumAttempts())
}

// ptr returns a pointer to v, used to populate optional pointer fields in the
// generated structs.
func ptr[T any](v T) *T {
	return &v
}
