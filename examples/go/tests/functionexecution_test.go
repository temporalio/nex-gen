package tests

import (
	"context"
	"reflect"
	"testing"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/suite"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

	"examples/go/functionexecution"
)

const functionExecutionServiceName = "FunctionExecution"

func validFunction(name string, enabled bool) string {
	return name
}

func validCountedFunction(name string, count int32) string {
	return name
}

func validVarargsFunction(args ...string) string {
	if len(args) == 0 {
		return ""
	}
	return args[0]
}

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
		functionExecutionServiceName,
		nexus.NewOperationReference[any, functionexecution.ExecuteFunctionResult]("ExecuteFunction"),
		mock.Anything,
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[functionexecution.ExecuteFunctionResult]{
			Value: functionexecution.ExecuteFunctionResult{Value: "one,true"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteFunctionResult, error) {
		return getFutureResult[functionexecution.ExecuteFunctionResult](
			ctx,
			functionexecution.ExecuteFunction(ctx, functionexecution.ExecuteFunctionOptions{}, validFunction, "one", true),
		)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("one,true", result.Value)
}

func (s *FunctionExecutionTestSuite) TestExecuteVarargsFunction() {
	s.env.OnNexusOperation(
		functionExecutionServiceName,
		nexus.NewOperationReference[any, functionexecution.ExecuteVarargsFunctionResult]("ExecuteVarargsFunction"),
		mock.Anything,
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[functionexecution.ExecuteVarargsFunctionResult]{
			Value: functionexecution.ExecuteVarargsFunctionResult{Value: "one,two"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteVarargsFunctionResult, error) {
		return getFutureResult[functionexecution.ExecuteVarargsFunctionResult](ctx, functionexecution.ExecuteVarargsFunction(
			ctx,
			functionexecution.ExecuteVarargsFunctionOptions{},
			validVarargsFunction,
			"one",
			"two",
		))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteVarargsFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("one,two", result.Value)
}

func (s *FunctionExecutionTestSuite) TestExecuteFunctionError() {
	s.env.OnNexusOperation(
		functionExecutionServiceName,
		nexus.NewOperationReference[any, functionexecution.ExecuteFunctionResult]("ExecuteFunction"),
		mock.Anything,
		mock.Anything,
	).Return(
		nil,
		nexus.NewOperationFailedError("function not found"),
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteFunctionResult, error) {
		return getFutureResult[functionexecution.ExecuteFunctionResult](
			ctx,
			functionexecution.ExecuteFunction(ctx, functionexecution.ExecuteFunctionOptions{}, validFunction, "one", false),
		)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.Error(s.env.GetWorkflowError())
}

func (s *FunctionExecutionTestSuite) TestOperationFutureCanBeSelected() {
	s.env.OnNexusOperation(
		functionExecutionServiceName,
		nexus.NewOperationReference[any, functionexecution.ExecuteFunctionResult]("ExecuteFunction"),
		mock.Anything,
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[functionexecution.ExecuteFunctionResult]{
			Value: functionexecution.ExecuteFunctionResult{Value: "selected"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteFunctionResult, error) {
		future := functionexecution.ExecuteFunction(
			ctx,
			functionexecution.ExecuteFunctionOptions{},
			validFunction,
			"one",
			true,
		)
		var result functionexecution.ExecuteFunctionResult
		var resultErr error
		workflow.NewSelector(ctx).AddFuture(future, func(ready workflow.Future) {
			resultErr = ready.Get(ctx, &result)
		}).Select(ctx)
		return &result, resultErr
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("selected", result.Value)
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

	executeFunction := nexus.NewSyncOperation("ExecuteFunction",
		func(ctx context.Context, input any, opts nexus.StartOperationOptions) (functionexecution.ExecuteFunctionResult, error) {
			s.calls = append(s.calls, testCall{"ExecuteFunction", input})
			return functionexecution.ExecuteFunctionResult{Value: "executed"}, nil
		})

	executeCountedFunction := nexus.NewSyncOperation("ExecuteCountedFunction",
		func(ctx context.Context, input any, opts nexus.StartOperationOptions) (functionexecution.ExecuteCountedFunctionResult, error) {
			s.calls = append(s.calls, testCall{"ExecuteCountedFunction", input})
			return functionexecution.ExecuteCountedFunctionResult{Value: "counted"}, nil
		})

	executeNamedFunction := nexus.NewSyncOperation("ExecuteNamedFunction",
		func(ctx context.Context, input any, opts nexus.StartOperationOptions) (functionexecution.ExecuteNamedFunctionResult, error) {
			s.calls = append(s.calls, testCall{"ExecuteNamedFunction", input})
			return functionexecution.ExecuteNamedFunctionResult{Value: "named"}, nil
		})

	executeVarargsFunction := nexus.NewSyncOperation("ExecuteVarargsFunction",
		func(ctx context.Context, input any, opts nexus.StartOperationOptions) (functionexecution.ExecuteVarargsFunctionResult, error) {
			s.calls = append(s.calls, testCall{"ExecuteVarargsFunction", input})
			return functionexecution.ExecuteVarargsFunctionResult{Value: "varargs"}, nil
		})

	executeNamedVarargsFunction := nexus.NewSyncOperation("ExecuteNamedVarargsFunction",
		func(ctx context.Context, input any, opts nexus.StartOperationOptions) (functionexecution.ExecuteNamedVarargsFunctionResult, error) {
			s.calls = append(s.calls, testCall{"ExecuteNamedVarargsFunction", input})
			return functionexecution.ExecuteNamedVarargsFunctionResult{Value: "named-varargs"}, nil
		})

	service := nexus.NewService(functionExecutionServiceName)
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
		return getFutureResult[functionexecution.ExecuteFunctionResult](
			ctx,
			functionexecution.ExecuteFunction(ctx, functionexecution.ExecuteFunctionOptions{}, validFunction, "one", true),
		)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("executed", result.Value)

	s.Require().Len(s.calls, 1)
	s.Equal("ExecuteFunction", s.calls[0].Operation)
}

func (s *FunctionExecutionIntegrationSuite) TestExecuteCountedFunction() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteCountedFunctionResult, error) {
		return getFutureResult[functionexecution.ExecuteCountedFunctionResult](
			ctx,
			functionexecution.ExecuteCountedFunction(ctx, functionexecution.ExecuteCountedFunctionOptions{}, validCountedFunction, "one", 7),
		)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteCountedFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("counted", result.Value)

	s.Require().Len(s.calls, 1)
	s.Equal("ExecuteCountedFunction", s.calls[0].Operation)
}

func (s *FunctionExecutionIntegrationSuite) TestExecuteNamedFunction() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteNamedFunctionResult, error) {
		return getFutureResult[functionexecution.ExecuteNamedFunctionResult](
			ctx,
			functionexecution.ExecuteNamedFunction(ctx, functionexecution.ExecuteNamedFunctionOptions{}, validFunction, "one", true),
		)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteNamedFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("named", result.Value)

	s.Require().Len(s.calls, 1)
	s.Equal("ExecuteNamedFunction", s.calls[0].Operation)
}

func (s *FunctionExecutionIntegrationSuite) TestExecuteVarargsFunction() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteVarargsFunctionResult, error) {
		return getFutureResult[functionexecution.ExecuteVarargsFunctionResult](ctx, functionexecution.ExecuteVarargsFunction(
			ctx,
			functionexecution.ExecuteVarargsFunctionOptions{},
			validVarargsFunction,
			"one",
			"two",
		))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteVarargsFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("varargs", result.Value)

	s.Require().Len(s.calls, 1)
	s.Equal("ExecuteVarargsFunction", s.calls[0].Operation)
	s.Equal("validVarargsFunction", stringField(s.calls[0].Input, "Function"))
	s.Equal([]string{"one", "two"}, stringSliceField(s.calls[0].Input, "Args"))
}

func (s *FunctionExecutionIntegrationSuite) TestExecuteNamedVarargsFunction() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteNamedVarargsFunctionResult, error) {
		return getFutureResult[functionexecution.ExecuteNamedVarargsFunctionResult](ctx, functionexecution.ExecuteNamedVarargsFunction(
			ctx,
			functionexecution.ExecuteNamedVarargsFunctionOptions{},
			"named-varargs-function",
			"one",
			"two",
		))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteNamedVarargsFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("named-varargs", result.Value)

	s.Require().Len(s.calls, 1)
	s.Equal("ExecuteNamedVarargsFunction", s.calls[0].Operation)
	s.Equal("named-varargs-function", stringField(s.calls[0].Input, "Function"))
	s.Equal([]string{"one", "two"}, stringSliceField(s.calls[0].Input, "Args"))
}

func (s *FunctionExecutionIntegrationSuite) TestExecuteNamedVarargsFunctionFunctionPointer() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*functionexecution.ExecuteNamedVarargsFunctionResult, error) {
		return getFutureResult[functionexecution.ExecuteNamedVarargsFunctionResult](ctx, functionexecution.ExecuteNamedVarargsFunction(
			ctx,
			functionexecution.ExecuteNamedVarargsFunctionOptions{},
			validVarargsFunction,
			"one",
			"two",
		))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result functionexecution.ExecuteNamedVarargsFunctionResult
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("named-varargs", result.Value)

	s.Require().Len(s.calls, 1)
	s.Equal("ExecuteNamedVarargsFunction", s.calls[0].Operation)
	s.Equal("validVarargsFunction", stringField(s.calls[0].Input, "Function"))
	s.Equal([]string{"one", "two"}, stringSliceField(s.calls[0].Input, "Args"))
}

func stringSliceField(value any, name string) []string {
	reflected := reflect.ValueOf(value)
	if reflected.Kind() == reflect.Pointer {
		reflected = reflected.Elem()
	}
	if reflected.Kind() == reflect.Struct {
		field := reflected.FieldByName(name)
		if values := stringSliceValue(field); values != nil {
			return values
		}
	}
	if reflected.Kind() == reflect.Map && reflected.Type().Key().Kind() == reflect.String {
		mapValue := reflected.MapIndex(reflect.ValueOf(name))
		if values := stringSliceValue(mapValue); values != nil {
			return values
		}
	}
	return nil
}

func stringSliceValue(value reflect.Value) []string {
	if !value.IsValid() {
		return nil
	}
	for value.Kind() == reflect.Interface || value.Kind() == reflect.Pointer {
		if value.IsNil() {
			return nil
		}
		value = value.Elem()
	}
	if value.Kind() != reflect.Slice {
		return nil
	}
	values := make([]string, 0, value.Len())
	for i := 0; i < value.Len(); i++ {
		item := value.Index(i)
		for item.Kind() == reflect.Interface || item.Kind() == reflect.Pointer {
			if item.IsNil() {
				item = reflect.Value{}
				break
			}
			item = item.Elem()
		}
		if !item.IsValid() {
			values = append(values, "")
			continue
		}
		if item.Kind() != reflect.String {
			return nil
		}
		values = append(values, item.String())
	}
	return values
}
