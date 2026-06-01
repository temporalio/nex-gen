package tests

import (
	"context"
	"testing"
	"time"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/suite"
	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

	tr "examples/go/typeroundtrip"
)

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

func (s *TypeRoundtripTestSuite) TestRetryPolicyOperation() {
	policy := temporal.RetryPolicy{MaximumAttempts: 3}

	s.env.OnNexusOperation(
		tr.ServiceName,
		nexus.NewOperationReference[temporal.RetryPolicy, temporal.RetryPolicy](tr.RetryPolicyOperationOp),
		policy,
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[temporal.RetryPolicy]{
			Value: policy,
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*temporal.RetryPolicy, error) {
		return tr.RetryPolicyOperation(ctx, policy)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result temporal.RetryPolicy
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal(int32(3), result.MaximumAttempts)
}

func (s *TypeRoundtripTestSuite) TestActivityOptionsOperation() {
	policy := temporal.RetryPolicy{MaximumAttempts: 3}
	priority := temporal.Priority{
		PriorityKey:    4,
		FairnessKey:    "tenant-a",
		FairnessWeight: 2.5,
	}

	s.env.OnNexusOperation(
		tr.ServiceName,
		nexus.NewOperationReference[tr.ActivityOptions, tr.ActivityOptions](tr.ActivityOptionsOperationOp),
		tr.ActivityOptions{
			TaskQueue:              "demo-task-queue",
			RetryPolicy:            policy,
			ScheduleToCloseTimeout: 7 * time.Second,
			Priority:               priority,
		},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[tr.ActivityOptions]{
			Value: tr.ActivityOptions{
				TaskQueue:              "demo-task-queue",
				RetryPolicy:            policy,
				ScheduleToCloseTimeout: 7 * time.Second,
				Priority:               priority,
			},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*tr.ActivityOptions, error) {
		return tr.ActivityOptionsOperation(ctx, policy, tr.ActivityOptionsOperationOptions{
			TaskQueue:              "demo-task-queue",
			ScheduleToCloseTimeout: 7 * time.Second,
			Priority:               priority,
		})
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result tr.ActivityOptions
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal(int32(3), result.RetryPolicy.MaximumAttempts)
	s.Equal("demo-task-queue", result.TaskQueue)
	s.Equal(7*time.Second, result.ScheduleToCloseTimeout)
	s.Equal(4, result.Priority.PriorityKey)
	s.Equal("tenant-a", result.Priority.FairnessKey)
	s.InDelta(2.5, float64(result.Priority.FairnessWeight), 0.01)
}

func (s *TypeRoundtripTestSuite) TestActivityOptionsOperationRequiredOnly() {
	policy := temporal.RetryPolicy{MaximumAttempts: 5}

	s.env.OnNexusOperation(
		tr.ServiceName,
		nexus.NewOperationReference[tr.ActivityOptions, tr.ActivityOptions](tr.ActivityOptionsOperationOp),
		tr.ActivityOptions{RetryPolicy: policy},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[tr.ActivityOptions]{
			Value: tr.ActivityOptions{RetryPolicy: policy},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*tr.ActivityOptions, error) {
		return tr.ActivityOptionsOperation(ctx, policy, tr.ActivityOptionsOperationOptions{})
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result tr.ActivityOptions
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal(int32(5), result.RetryPolicy.MaximumAttempts)
	s.Equal("", result.TaskQueue)
	s.Equal(time.Duration(0), result.ScheduleToCloseTimeout)
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

	retryPolicyOp := nexus.NewSyncOperation(tr.RetryPolicyOperationOp,
		func(ctx context.Context, input temporal.RetryPolicy, opts nexus.StartOperationOptions) (temporal.RetryPolicy, error) {
			s.calls = append(s.calls, testCall{"RetryPolicyOperation", input})
			return input, nil
		})

	activityOptionsOp := nexus.NewSyncOperation(tr.ActivityOptionsOperationOp,
		func(ctx context.Context, input tr.ActivityOptions, opts nexus.StartOperationOptions) (tr.ActivityOptions, error) {
			s.calls = append(s.calls, testCall{"ActivityOptionsOperation", input})
			return input, nil
		})

	service := nexus.NewService(tr.ServiceName)
	s.NoError(service.Register(retryPolicyOp, activityOptionsOp))
	s.env.RegisterNexusService(service)
}

func TestTypeRoundtripIntegrationSuite(t *testing.T) {
	suite.Run(t, new(TypeRoundtripIntegrationSuite))
}

func (s *TypeRoundtripIntegrationSuite) TestRetryPolicyOperation() {
	policy := temporal.RetryPolicy{MaximumAttempts: 3}

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*temporal.RetryPolicy, error) {
		return tr.RetryPolicyOperation(ctx, policy)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result temporal.RetryPolicy
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal(int32(3), result.MaximumAttempts)

	s.Require().Len(s.calls, 1)
	s.Equal("RetryPolicyOperation", s.calls[0].Operation)
	handlerInput := s.calls[0].Input.(temporal.RetryPolicy)
	s.Equal(int32(3), handlerInput.MaximumAttempts)
}

func (s *TypeRoundtripIntegrationSuite) TestActivityOptionsOperation() {
	policy := temporal.RetryPolicy{MaximumAttempts: 3}
	priority := temporal.Priority{
		PriorityKey:    4,
		FairnessKey:    "tenant-a",
		FairnessWeight: 2.5,
	}

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*tr.ActivityOptions, error) {
		return tr.ActivityOptionsOperation(ctx, policy, tr.ActivityOptionsOperationOptions{
			TaskQueue:              "demo-task-queue",
			ScheduleToCloseTimeout: 7 * time.Second,
			Priority:               priority,
		})
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result tr.ActivityOptions
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal(int32(3), result.RetryPolicy.MaximumAttempts)
	s.Equal("demo-task-queue", result.TaskQueue)
	s.Equal(7*time.Second, result.ScheduleToCloseTimeout)
	s.Equal(4, result.Priority.PriorityKey)
	s.Equal("tenant-a", result.Priority.FairnessKey)
	s.InDelta(2.5, float64(result.Priority.FairnessWeight), 0.01)

	s.Require().Len(s.calls, 1)
	s.Equal("ActivityOptionsOperation", s.calls[0].Operation)
	handlerInput := s.calls[0].Input.(tr.ActivityOptions)
	s.Equal(int32(3), handlerInput.RetryPolicy.MaximumAttempts)
	s.Equal("demo-task-queue", handlerInput.TaskQueue)
	s.Equal(7*time.Second, handlerInput.ScheduleToCloseTimeout)
	s.Equal(4, handlerInput.Priority.PriorityKey)
}

func (s *TypeRoundtripIntegrationSuite) TestActivityOptionsOperationRequiredOnly() {
	policy := temporal.RetryPolicy{MaximumAttempts: 5}

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*tr.ActivityOptions, error) {
		return tr.ActivityOptionsOperation(ctx, policy, tr.ActivityOptionsOperationOptions{})
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result tr.ActivityOptions
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal(int32(5), result.RetryPolicy.MaximumAttempts)
	s.Equal("", result.TaskQueue)
	s.Equal(time.Duration(0), result.ScheduleToCloseTimeout)

	s.Require().Len(s.calls, 1)
	s.Equal("ActivityOptionsOperation", s.calls[0].Operation)
	handlerInput := s.calls[0].Input.(tr.ActivityOptions)
	s.Equal(int32(5), handlerInput.RetryPolicy.MaximumAttempts)
}
