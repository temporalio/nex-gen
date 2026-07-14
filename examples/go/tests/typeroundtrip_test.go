package tests

import (
	"context"
	"testing"
	"time"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/suite"
	activitypb "go.temporal.io/api/activity/v1"
	commonpb "go.temporal.io/api/common/v1"
	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

	tr "examples/go/typeroundtrip"
)

const typeRoundtripServiceName = "TypeRoundtripService"

type TypeRoundtripIntegrationSuite struct {
	suite.Suite
	testsuite.WorkflowTestSuite
	env   *testsuite.TestWorkflowEnvironment
	calls []nexusCall
}

func (s *TypeRoundtripIntegrationSuite) SetupTest() {
	s.env = s.NewTestWorkflowEnvironment()
	s.calls = nil

	retryPolicyOp := nexus.NewSyncOperation("RetryPolicyOperation",
		func(ctx context.Context, input *commonpb.RetryPolicy, opts nexus.StartOperationOptions) (*commonpb.RetryPolicy, error) {
			s.calls = append(s.calls, nexusCall{"RetryPolicyOperation", input})
			return input, nil
		})

	activityOptionsOp := nexus.NewSyncOperation("ActivityOptionsOperation",
		func(ctx context.Context, input *activitypb.ActivityOptions, opts nexus.StartOperationOptions) (*activitypb.ActivityOptions, error) {
			s.calls = append(s.calls, nexusCall{"ActivityOptionsOperation", input})
			return input, nil
		})

	service := nexus.NewService(typeRoundtripServiceName)
	s.NoError(service.Register(retryPolicyOp, activityOptionsOp))
	s.env.RegisterNexusService(service)
}

func TestTypeRoundtripIntegrationSuite(t *testing.T) {
	suite.Run(t, new(TypeRoundtripIntegrationSuite))
}

type typeRoundtripResults struct {
	RetryPolicy    temporal.RetryPolicy
	ActivityOption tr.ActivityOptions
}

func (s *TypeRoundtripIntegrationSuite) TestProtoRoundTrips() {
	policy := temporal.RetryPolicy{MaximumAttempts: 3}
	priority := temporal.Priority{
		PriorityKey:    4,
		FairnessKey:    "tenant-a",
		FairnessWeight: 2.5,
	}

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*typeRoundtripResults, error) {
		var results typeRoundtripResults
		// Public Temporal SDK types are converted to protobuf at the Nexus
		// boundary, then converted back when OperationFuture.Get resolves.
		if err := tr.RetryPolicyOperation(
			ctx, tr.RetryPolicyOperationOptions{}, policy,
		).Get(ctx, &results.RetryPolicy); err != nil {
			return nil, err
		}
		if err := tr.ActivityOptionsOperation(ctx, tr.ActivityOptionsOperationOptions{
			RetryPolicy:            policy,
			TaskQueue:              "demo-task-queue",
			ScheduleToCloseTimeout: 7 * time.Second,
			Priority:               &priority,
		}).Get(ctx, &results.ActivityOption); err != nil {
			return nil, err
		}
		return &results, nil
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var results typeRoundtripResults
	s.NoError(s.env.GetWorkflowResult(&results))
	s.Equal(int32(3), results.RetryPolicy.MaximumAttempts)
	s.Equal(int32(3), results.ActivityOption.RetryPolicy.MaximumAttempts)
	s.Require().NotNil(results.ActivityOption.TaskQueue)
	s.Equal("demo-task-queue", *results.ActivityOption.TaskQueue)
	s.Require().NotNil(results.ActivityOption.ScheduleToCloseTimeout)
	s.Equal(7*time.Second, *results.ActivityOption.ScheduleToCloseTimeout)
	s.Require().NotNil(results.ActivityOption.Priority)
	s.Equal(4, results.ActivityOption.Priority.PriorityKey)
	s.Equal("tenant-a", results.ActivityOption.Priority.FairnessKey)
	s.InDelta(2.5, float64(results.ActivityOption.Priority.FairnessWeight), 0.01)

	s.Require().Len(s.calls, 2)
	s.Equal([]string{"RetryPolicyOperation", "ActivityOptionsOperation"}, operationNames(s.calls))
	retryInput := s.calls[0].Input.(*commonpb.RetryPolicy)
	s.Equal(int32(3), retryInput.GetMaximumAttempts())
	activityInput := s.calls[1].Input.(*activitypb.ActivityOptions)
	s.Equal(int32(3), activityInput.GetRetryPolicy().GetMaximumAttempts())
	s.Equal("demo-task-queue", activityInput.GetTaskQueue().GetName())
	s.Equal(7*time.Second, activityInput.GetScheduleToCloseTimeout().AsDuration())
	s.Equal(int32(4), activityInput.GetPriority().GetPriorityKey())
}

func (s *TypeRoundtripIntegrationSuite) TestActivityOptionsOperationRequiredOnly() {
	policy := temporal.RetryPolicy{MaximumAttempts: 5}

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*tr.ActivityOptions, error) {
		var result tr.ActivityOptions
		return &result, tr.ActivityOptionsOperation(ctx, tr.ActivityOptionsOperationOptions{RetryPolicy: policy}).Get(ctx, &result)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result tr.ActivityOptions
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal(int32(5), result.RetryPolicy.MaximumAttempts)
	// Optional fields that were never supplied remain absent after the round
	// trip, which is distinct from present fields containing their zero value.
	s.Nil(result.TaskQueue)
	s.Nil(result.ScheduleToCloseTimeout)
	s.Nil(result.Priority)

	s.Require().Len(s.calls, 1)
	s.Equal("ActivityOptionsOperation", s.calls[0].Operation)
	handlerInput := s.calls[0].Input.(*activitypb.ActivityOptions)
	s.Equal(int32(5), handlerInput.GetRetryPolicy().GetMaximumAttempts())
}
