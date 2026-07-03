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

	"examples/go/userservice"
)

const userServiceName = "UserService"

// --- Mock tests ---

type UserServiceTestSuite struct {
	suite.Suite
	testsuite.WorkflowTestSuite
	env *testsuite.TestWorkflowEnvironment
}

func (s *UserServiceTestSuite) SetupTest() {
	s.env = s.NewTestWorkflowEnvironment()
}

func (s *UserServiceTestSuite) AfterTest(suiteName, testName string) {
	s.env.AssertExpectations(s.T())
}

func TestUserServiceSuite(t *testing.T) {
	suite.Run(t, new(UserServiceTestSuite))
}

func (s *UserServiceTestSuite) TestGetUser() {
	s.env.OnNexusOperation(
		userServiceName,
		nexus.NewOperationReference[any, userservice.User]("GetUser"),
		mock.Anything,
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[userservice.User]{
			Value: userservice.User{UserId: "user-123", Email: "alice@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		return getFutureResult[userservice.User](ctx, userservice.GetUser(ctx, "user-123"))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result userservice.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("alice@example.com", result.Email)
}

func (s *UserServiceTestSuite) TestUpdateEmail() {
	s.env.OnNexusOperation(
		userServiceName,
		nexus.NewOperationReference[any, userservice.User]("UpdateEmail"),
		mock.Anything,
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[userservice.User]{
			Value: userservice.User{UserId: "user-123", Email: "new@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		return getFutureResult[userservice.User](
			ctx,
			userservice.UpdateEmail(ctx, "user-123", "new@example.com"),
		)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result userservice.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("new@example.com", result.Email)
}

func (s *UserServiceTestSuite) TestUserUpdateEmailMethod() {
	s.env.OnNexusOperation(
		userServiceName,
		nexus.NewOperationReference[any, userservice.User]("UpdateEmail"),
		mock.Anything,
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[userservice.User]{
			Value: userservice.User{UserId: "user-123", Email: "updated@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		user := &userservice.User{UserId: "user-123", Email: "old@example.com"}
		return getFutureResult[userservice.User](ctx, user.UpdateEmail(ctx, "updated@example.com"))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result userservice.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("updated@example.com", result.Email)
}

func (s *UserServiceTestSuite) TestGetUserThenUpdateEmail() {
	s.env.OnNexusOperation(
		userServiceName,
		nexus.NewOperationReference[any, userservice.User]("GetUser"),
		mock.Anything,
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[userservice.User]{
			Value: userservice.User{UserId: "user-123", Email: "old@example.com"},
		},
		nil,
	)

	s.env.OnNexusOperation(
		userServiceName,
		nexus.NewOperationReference[any, userservice.User]("UpdateEmail"),
		mock.Anything,
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[userservice.User]{
			Value: userservice.User{UserId: "user-123", Email: "new@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		user, err := getFutureResult[userservice.User](ctx, userservice.GetUser(ctx, "user-123"))
		if err != nil {
			return nil, err
		}
		return getFutureResult[userservice.User](ctx, user.UpdateEmail(ctx, "new@example.com"))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result userservice.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("new@example.com", result.Email)
}

func (s *UserServiceTestSuite) TestGetUserError() {
	s.env.OnNexusOperation(
		userServiceName,
		nexus.NewOperationReference[any, userservice.User]("GetUser"),
		mock.Anything,
		mock.Anything,
	).Return(
		nil,
		nexus.NewOperationFailedError("user not found"),
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		return getFutureResult[userservice.User](ctx, userservice.GetUser(ctx, "nonexistent"))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.Error(s.env.GetWorkflowError())
}

// --- Integration tests ---

type testCall struct {
	Operation string
	Input     any
}

type UserServiceIntegrationSuite struct {
	suite.Suite
	testsuite.WorkflowTestSuite
	env   *testsuite.TestWorkflowEnvironment
	calls []testCall
}

func (s *UserServiceIntegrationSuite) SetupTest() {
	s.env = s.NewTestWorkflowEnvironment()
	s.calls = nil

	getUser := nexus.NewSyncOperation("GetUser",
		func(ctx context.Context, input any, opts nexus.StartOperationOptions) (userservice.User, error) {
			s.calls = append(s.calls, testCall{"GetUser", input})
			return userservice.User{UserId: stringField(input, "UserId"), Email: "alice@example.com"}, nil
		})

	updateEmail := nexus.NewSyncOperation("UpdateEmail",
		func(ctx context.Context, input any, opts nexus.StartOperationOptions) (userservice.User, error) {
			s.calls = append(s.calls, testCall{"UpdateEmail", input})
			return userservice.User{UserId: stringField(input, "UserId"), Email: stringField(input, "Email")}, nil
		})

	service := nexus.NewService(userServiceName)
	s.NoError(service.Register(getUser, updateEmail))
	s.env.RegisterNexusService(service)
}

func TestUserServiceIntegrationSuite(t *testing.T) {
	suite.Run(t, new(UserServiceIntegrationSuite))
}

func (s *UserServiceIntegrationSuite) TestGetUser() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		return getFutureResult[userservice.User](ctx, userservice.GetUser(ctx, "user-123"))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result userservice.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("alice@example.com", result.Email)

	s.Require().Len(s.calls, 1)
	s.Equal("GetUser", s.calls[0].Operation)
}

func (s *UserServiceIntegrationSuite) TestUpdateEmail() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		return getFutureResult[userservice.User](
			ctx,
			userservice.UpdateEmail(ctx, "user-123", "new@example.com"),
		)
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result userservice.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("new@example.com", result.Email)

	s.Require().Len(s.calls, 1)
	s.Equal("UpdateEmail", s.calls[0].Operation)
}

func (s *UserServiceIntegrationSuite) TestUserUpdateEmailMethod() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		user := &userservice.User{UserId: "user-123", Email: "old@example.com"}
		return getFutureResult[userservice.User](ctx, user.UpdateEmail(ctx, "updated@example.com"))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result userservice.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("updated@example.com", result.Email)

	s.Require().Len(s.calls, 1)
	s.Equal("UpdateEmail", s.calls[0].Operation)
}

func (s *UserServiceIntegrationSuite) TestGetUserThenUpdateEmail() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		user, err := getFutureResult[userservice.User](ctx, userservice.GetUser(ctx, "user-123"))
		if err != nil {
			return nil, err
		}
		return getFutureResult[userservice.User](ctx, user.UpdateEmail(ctx, "new@example.com"))
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result userservice.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("new@example.com", result.Email)

	s.Require().Len(s.calls, 2)
	s.Equal("GetUser", s.calls[0].Operation)
	s.Equal("UpdateEmail", s.calls[1].Operation)
}

func stringField(value any, name string) string {
	reflected := reflect.ValueOf(value)
	if reflected.Kind() == reflect.Pointer {
		reflected = reflected.Elem()
	}
	if reflected.Kind() == reflect.Struct {
		field := reflected.FieldByName(name)
		if field.IsValid() && field.Kind() == reflect.String {
			return field.String()
		}
	}
	if reflected.Kind() == reflect.Map && reflected.Type().Key().Kind() == reflect.String {
		mapValue := reflected.MapIndex(reflect.ValueOf(name))
		if mapValue.IsValid() && mapValue.Kind() == reflect.Interface {
			mapValue = mapValue.Elem()
		}
		if mapValue.IsValid() && mapValue.Kind() == reflect.String {
			return mapValue.String()
		}
	}
	return ""
}
