package tests

import (
	"testing"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/suite"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

	us "examples/go/userservice"
)

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
		us.ServiceName,
		nexus.NewOperationReference[us.GetUserRequest, us.User](us.GetUserOp),
		us.GetUserRequest{UserId: "user-123"},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[us.User]{
			Value: us.User{UserId: "user-123", Email: "alice@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*us.User, error) {
		return us.GetUser(ctx, "user-123")
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result us.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("alice@example.com", result.Email)
}

func (s *UserServiceTestSuite) TestUpdateEmail() {
	s.env.OnNexusOperation(
		us.ServiceName,
		nexus.NewOperationReference[us.UpdateEmailRequest, us.User](us.UpdateEmailOp),
		us.UpdateEmailRequest{UserId: "user-123", Email: "new@example.com"},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[us.User]{
			Value: us.User{UserId: "user-123", Email: "new@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*us.User, error) {
		return us.UpdateEmail(ctx, "user-123", "new@example.com")
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result us.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("new@example.com", result.Email)
}

func (s *UserServiceTestSuite) TestUserUpdateEmailMethod() {
	s.env.OnNexusOperation(
		us.ServiceName,
		nexus.NewOperationReference[us.UpdateEmailRequest, us.User](us.UpdateEmailOp),
		us.UpdateEmailRequest{UserId: "user-123", Email: "updated@example.com"},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[us.User]{
			Value: us.User{UserId: "user-123", Email: "updated@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*us.User, error) {
		user := &us.User{UserId: "user-123", Email: "old@example.com"}
		return user.UpdateEmail(ctx, "updated@example.com")
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result us.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("updated@example.com", result.Email)
}

func (s *UserServiceTestSuite) TestGetUserThenUpdateEmail() {
	s.env.OnNexusOperation(
		us.ServiceName,
		nexus.NewOperationReference[us.GetUserRequest, us.User](us.GetUserOp),
		us.GetUserRequest{UserId: "user-123"},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[us.User]{
			Value: us.User{UserId: "user-123", Email: "old@example.com"},
		},
		nil,
	)

	s.env.OnNexusOperation(
		us.ServiceName,
		nexus.NewOperationReference[us.UpdateEmailRequest, us.User](us.UpdateEmailOp),
		us.UpdateEmailRequest{UserId: "user-123", Email: "new@example.com"},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[us.User]{
			Value: us.User{UserId: "user-123", Email: "new@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*us.User, error) {
		user, err := us.GetUser(ctx, "user-123")
		if err != nil {
			return nil, err
		}
		return user.UpdateEmail(ctx, "new@example.com")
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result us.User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("new@example.com", result.Email)
}

func (s *UserServiceTestSuite) TestGetUserError() {
	s.env.OnNexusOperation(
		us.ServiceName,
		nexus.NewOperationReference[us.GetUserRequest, us.User](us.GetUserOp),
		us.GetUserRequest{UserId: "nonexistent"},
		mock.Anything,
	).Return(
		nil,
		nexus.NewOperationFailedError("user not found"),
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*us.User, error) {
		return us.GetUser(ctx, "nonexistent")
	})

	s.True(s.env.IsWorkflowCompleted())
	s.Error(s.env.GetWorkflowError())
}
