package tests

import (
	"context"
	"testing"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/suite"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

	"examples/go/userservice"
)

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
		userservice.ServiceName,
		nexus.NewOperationReference[userservice.GetUserRequest, userservice.User](userservice.GetUserOp),
		userservice.GetUserRequest{UserId: "user-123"},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[userservice.User]{
			Value: userservice.User{UserId: "user-123", Email: "alice@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		return userservice.GetUser(ctx, "user-123")
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
		userservice.ServiceName,
		nexus.NewOperationReference[userservice.UpdateEmailRequest, userservice.User](userservice.UpdateEmailOp),
		userservice.UpdateEmailRequest{UserId: "user-123", Email: "new@example.com"},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[userservice.User]{
			Value: userservice.User{UserId: "user-123", Email: "new@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		return userservice.UpdateEmail(ctx, "user-123", "new@example.com")
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
		userservice.ServiceName,
		nexus.NewOperationReference[userservice.UpdateEmailRequest, userservice.User](userservice.UpdateEmailOp),
		userservice.UpdateEmailRequest{UserId: "user-123", Email: "updated@example.com"},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[userservice.User]{
			Value: userservice.User{UserId: "user-123", Email: "updated@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		user := &userservice.User{UserId: "user-123", Email: "old@example.com"}
		return user.UpdateEmail(ctx, "updated@example.com")
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
		userservice.ServiceName,
		nexus.NewOperationReference[userservice.GetUserRequest, userservice.User](userservice.GetUserOp),
		userservice.GetUserRequest{UserId: "user-123"},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[userservice.User]{
			Value: userservice.User{UserId: "user-123", Email: "old@example.com"},
		},
		nil,
	)

	s.env.OnNexusOperation(
		userservice.ServiceName,
		nexus.NewOperationReference[userservice.UpdateEmailRequest, userservice.User](userservice.UpdateEmailOp),
		userservice.UpdateEmailRequest{UserId: "user-123", Email: "new@example.com"},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[userservice.User]{
			Value: userservice.User{UserId: "user-123", Email: "new@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		user, err := userservice.GetUser(ctx, "user-123")
		if err != nil {
			return nil, err
		}
		return user.UpdateEmail(ctx, "new@example.com")
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
		userservice.ServiceName,
		nexus.NewOperationReference[userservice.GetUserRequest, userservice.User](userservice.GetUserOp),
		userservice.GetUserRequest{UserId: "nonexistent"},
		mock.Anything,
	).Return(
		nil,
		nexus.NewOperationFailedError("user not found"),
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		return userservice.GetUser(ctx, "nonexistent")
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

	getUser := nexus.NewSyncOperation(userservice.GetUserOp,
		func(ctx context.Context, input userservice.GetUserRequest, opts nexus.StartOperationOptions) (userservice.User, error) {
			s.calls = append(s.calls, testCall{"GetUser", input})
			return userservice.User{UserId: input.UserId, Email: "alice@example.com"}, nil
		})

	updateEmail := nexus.NewSyncOperation(userservice.UpdateEmailOp,
		func(ctx context.Context, input userservice.UpdateEmailRequest, opts nexus.StartOperationOptions) (userservice.User, error) {
			s.calls = append(s.calls, testCall{"UpdateEmail", input})
			return userservice.User{UserId: input.UserId, Email: input.Email}, nil
		})

	service := nexus.NewService(userservice.ServiceName)
	s.NoError(service.Register(getUser, updateEmail))
	s.env.RegisterNexusService(service)
}

func TestUserServiceIntegrationSuite(t *testing.T) {
	suite.Run(t, new(UserServiceIntegrationSuite))
}

func (s *UserServiceIntegrationSuite) TestGetUser() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		return userservice.GetUser(ctx, "user-123")
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result userservice.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("alice@example.com", result.Email)

	s.Require().Len(s.calls, 1)
	s.Equal("GetUser", s.calls[0].Operation)
	s.Equal(userservice.GetUserRequest{UserId: "user-123"}, s.calls[0].Input)
}

func (s *UserServiceIntegrationSuite) TestUpdateEmail() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		return userservice.UpdateEmail(ctx, "user-123", "new@example.com")
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result userservice.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("new@example.com", result.Email)

	s.Require().Len(s.calls, 1)
	s.Equal("UpdateEmail", s.calls[0].Operation)
	s.Equal(userservice.UpdateEmailRequest{UserId: "user-123", Email: "new@example.com"}, s.calls[0].Input)
}

func (s *UserServiceIntegrationSuite) TestUserUpdateEmailMethod() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		user := &userservice.User{UserId: "user-123", Email: "old@example.com"}
		return user.UpdateEmail(ctx, "updated@example.com")
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result userservice.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("updated@example.com", result.Email)

	s.Require().Len(s.calls, 1)
	s.Equal("UpdateEmail", s.calls[0].Operation)
	s.Equal(userservice.UpdateEmailRequest{UserId: "user-123", Email: "updated@example.com"}, s.calls[0].Input)
}

func (s *UserServiceIntegrationSuite) TestGetUserThenUpdateEmail() {
	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*userservice.User, error) {
		user, err := userservice.GetUser(ctx, "user-123")
		if err != nil {
			return nil, err
		}
		return user.UpdateEmail(ctx, "new@example.com")
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result userservice.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("new@example.com", result.Email)

	s.Require().Len(s.calls, 2)
	s.Equal("GetUser", s.calls[0].Operation)
	s.Equal(userservice.GetUserRequest{UserId: "user-123"}, s.calls[0].Input)
	s.Equal("UpdateEmail", s.calls[1].Operation)
	s.Equal(userservice.UpdateEmailRequest{UserId: "user-123", Email: "new@example.com"}, s.calls[1].Input)
}
