package tests

import (
	"context"
	"testing"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/suite"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

	"examples/go/functionexecution"
)

// --- Mock tests ---

type FunctionExecutionTestSuite struct {
	suite.Suite
	testsuite.WorkflowTestSuite
	env *testsuite.TestWorkflowEnvironment
}

func (s *FunctionExecutionTestSuite) SetupTest() {
	s.env = s.NewTestWorkflowEnvironment()
}

func (s *FunctionExecutionTestSuite) AfterTest(suiteName, testName string) {
	s.env.AssertExpectations(s.T())
}

func TestFunctionExecutionSuite(t *testing.T) {
	suite.Run(t, new(FunctionExecutionTestSuite))
}

func (s *FunctionExecutionTestSuite) TestExecuteFunction() {
	s.env.OnNexusOperation(
		functionexecution.ServiceName,
		nexus.NewOperationReference[functionexecution.ExecuteFunctionRequest, functionexecution.ExecuteFunctionResult](functionexecution.ExecuteFunctionOp),
		functionexecution.ExecuteFunctionRequest{Function: "valid-function", Name: "one", Enabled: true},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[functionexecution.ExecuteFunctionResult]{
			Value: functionexecution.ExecuteFunctionResult{Value: "one,true"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteFunctionResult, error) {
		return functionexecution.ExecuteFunction(ctx, "valid-function", "one", true)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("one,true", result.Value)
}

func (s *FunctionExecutionTestSuite) TestExecuteVarargsFunction() {
	s.env.OnNexusOperation(
		functionexecution.ServiceName,
		nexus.NewOperationReference[functionexecution.ExecuteVarargsFunctionRequest, functionexecution.ExecuteVarargsFunctionResult](functionexecution.ExecuteVarargsFunctionOp),
		functionexecution.ExecuteVarargsFunctionRequest{Function: "valid-varargs-function", Args: []string{"one", "two"}},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[functionexecution.ExecuteVarargsFunctionResult]{
			Value: functionexecution.ExecuteVarargsFunctionResult{Value: "one,two"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteVarargsFunctionResult, error) {
		return functionexecution.ExecuteVarargsFunction(
			ctx,
			"valid-varargs-function",
			functionexecution.ExecuteVarargsFunctionOptions{Args: []string{"one", "two"}},
		)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteVarargsFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("one,two", result.Value)
}

func (s *FunctionExecutionTestSuite) TestExecuteFunctionError() {
	s.env.OnNexusOperation(
		functionexecution.ServiceName,
		nexus.NewOperationReference[functionexecution.ExecuteFunctionRequest, functionexecution.ExecuteFunctionResult](functionexecution.ExecuteFunctionOp),
		functionexecution.ExecuteFunctionRequest{Function: "missing-function", Name: "one", Enabled: false},
		mock.Anything,
	).Return(
		nil,
		nexus.NewOperationFailedError("function not found"),
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteFunctionResult, error) {
		return functionexecution.ExecuteFunction(ctx, "missing-function", "one", false)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.Error(s.env.GetWorkflowError())
}

// --- Integration tests ---

type FunctionExecutionIntegrationSuite struct {
	suite.Suite
	testsuite.WorkflowTestSuite
	env   *testsuite.TestWorkflowEnvironment
	calls []testCall
}

func (s *FunctionExecutionIntegrationSuite) SetupTest() {
	s.env = s.NewTestWorkflowEnvironment()
	s.calls = nil

	executeFunction := nexus.NewSyncOperation(functionexecution.ExecuteFunctionOp,
		func(ctx context.Context, input functionexecution.ExecuteFunctionRequest, opts nexus.StartOperationOptions) (functionexecution.ExecuteFunctionResult, error) {
			s.calls = append(s.calls, testCall{"ExecuteFunction", input})
			return functionexecution.ExecuteFunctionResult{Value: "executed"}, nil
		})

	executeCountedFunction := nexus.NewSyncOperation(functionexecution.ExecuteCountedFunctionOp,
		func(ctx context.Context, input functionexecution.ExecuteCountedFunctionRequest, opts nexus.StartOperationOptions) (functionexecution.ExecuteCountedFunctionResult, error) {
			s.calls = append(s.calls, testCall{"ExecuteCountedFunction", input})
			return functionexecution.ExecuteCountedFunctionResult{Value: "counted"}, nil
		})

	executeNamedFunction := nexus.NewSyncOperation(functionexecution.ExecuteNamedFunctionOp,
		func(ctx context.Context, input functionexecution.ExecuteNamedFunctionRequest, opts nexus.StartOperationOptions) (functionexecution.ExecuteNamedFunctionResult, error) {
			s.calls = append(s.calls, testCall{"ExecuteNamedFunction", input})
			return functionexecution.ExecuteNamedFunctionResult{Value: "named"}, nil
		})

	executeVarargsFunction := nexus.NewSyncOperation(functionexecution.ExecuteVarargsFunctionOp,
		func(ctx context.Context, input functionexecution.ExecuteVarargsFunctionRequest, opts nexus.StartOperationOptions) (functionexecution.ExecuteVarargsFunctionResult, error) {
			s.calls = append(s.calls, testCall{"ExecuteVarargsFunction", input})
			return functionexecution.ExecuteVarargsFunctionResult{Value: "varargs"}, nil
		})

	executeNamedVarargsFunction := nexus.NewSyncOperation(functionexecution.ExecuteNamedVarargsFunctionOp,
		func(ctx context.Context, input functionexecution.ExecuteNamedVarargsFunctionRequest, opts nexus.StartOperationOptions) (functionexecution.ExecuteNamedVarargsFunctionResult, error) {
			s.calls = append(s.calls, testCall{"ExecuteNamedVarargsFunction", input})
			return functionexecution.ExecuteNamedVarargsFunctionResult{Value: "named-varargs"}, nil
		})

	service := nexus.NewService(functionexecution.ServiceName)
	s.NoError(service.Register(
		executeFunction,
		executeCountedFunction,
		executeNamedFunction,
		executeVarargsFunction,
		executeNamedVarargsFunction,
	))
	s.env.RegisterNexusService(service)
}

func TestFunctionExecutionIntegrationSuite(t *testing.T) {
	suite.Run(t, new(FunctionExecutionIntegrationSuite))
}

func (s *FunctionExecutionIntegrationSuite) TestExecuteFunction() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteFunctionResult, error) {
		return functionexecution.ExecuteFunction(ctx, "valid-function", "one", true)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("executed", result.Value)

	s.Require().Len(s.calls, 1)
	s.Equal("ExecuteFunction", s.calls[0].Operation)
	s.Equal(functionexecution.ExecuteFunctionRequest{
		Function: "valid-function",
		Name:     "one",
		Enabled:  true,
	}, s.calls[0].Input)
}

func (s *FunctionExecutionIntegrationSuite) TestExecuteCountedFunction() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteCountedFunctionResult, error) {
		return functionexecution.ExecuteCountedFunction(ctx, "valid-counted-function", "one", 7)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteCountedFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("counted", result.Value)

	s.Require().Len(s.calls, 1)
	s.Equal("ExecuteCountedFunction", s.calls[0].Operation)
	s.Equal(functionexecution.ExecuteCountedFunctionRequest{
		Function: "valid-counted-function",
		Name:     "one",
		Count:    7,
	}, s.calls[0].Input)
}

func (s *FunctionExecutionIntegrationSuite) TestExecuteNamedFunction() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteNamedFunctionResult, error) {
		return functionexecution.ExecuteNamedFunction(ctx, "named-function", "one", true)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteNamedFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("named", result.Value)

	s.Require().Len(s.calls, 1)
	s.Equal("ExecuteNamedFunction", s.calls[0].Operation)
	s.Equal(functionexecution.ExecuteNamedFunctionRequest{
		Function: "named-function",
		Name:     "one",
		Enabled:  true,
	}, s.calls[0].Input)
}

func (s *FunctionExecutionIntegrationSuite) TestExecuteVarargsFunction() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteVarargsFunctionResult, error) {
		return functionexecution.ExecuteVarargsFunction(
			ctx,
			"valid-varargs-function",
			functionexecution.ExecuteVarargsFunctionOptions{Args: []string{"one", "two"}},
		)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteVarargsFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("varargs", result.Value)

	s.Require().Len(s.calls, 1)
	s.Equal("ExecuteVarargsFunction", s.calls[0].Operation)
	s.Equal(functionexecution.ExecuteVarargsFunctionRequest{
		Function: "valid-varargs-function",
		Args:     []string{"one", "two"},
	}, s.calls[0].Input)
}

func (s *FunctionExecutionIntegrationSuite) TestExecuteNamedVarargsFunction() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteNamedVarargsFunctionResult, error) {
		return functionexecution.ExecuteNamedVarargsFunction(
			ctx,
			"named-varargs-function",
			functionexecution.ExecuteNamedVarargsFunctionOptions{Args: []string{"one", "two"}},
		)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteNamedVarargsFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("named-varargs", result.Value)

	s.Require().Len(s.calls, 1)
	s.Equal("ExecuteNamedVarargsFunction", s.calls[0].Operation)
	s.Equal(functionexecution.ExecuteNamedVarargsFunctionRequest{
		Function: "named-varargs-function",
		Args:     []string{"one", "two"},
	}, s.calls[0].Input)
}
