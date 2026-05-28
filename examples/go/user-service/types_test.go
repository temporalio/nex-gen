package userservice

import (
	"testing"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/suite"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"
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
		ServiceName,
		nexus.NewOperationReference[GetUserRequest, User](GetUserOp),
		GetUserRequest{UserId: "user-123"},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[User]{
			Value: User{UserId: "user-123", Email: "alice@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*User, error) {
		return GetUser(ctx, "user-123")
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("alice@example.com", result.Email)
}

func (s *UserServiceTestSuite) TestUpdateEmail() {
	s.env.OnNexusOperation(
		ServiceName,
		nexus.NewOperationReference[UpdateEmailRequest, User](UpdateEmailOp),
		UpdateEmailRequest{UserId: "user-123", Email: "new@example.com"},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[User]{
			Value: User{UserId: "user-123", Email: "new@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*User, error) {
		return UpdateEmail(ctx, "user-123", "new@example.com")
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("new@example.com", result.Email)
}

func (s *UserServiceTestSuite) TestUserUpdateEmailMethod() {
	// The resource method should construct UpdateEmailRequest with
	// UserId from the receiver and Email from the argument.
	s.env.OnNexusOperation(
		ServiceName,
		nexus.NewOperationReference[UpdateEmailRequest, User](UpdateEmailOp),
		UpdateEmailRequest{UserId: "user-123", Email: "updated@example.com"},
		mock.Anything,
	).Return(
		&nexus.HandlerStartOperationResultSync[User]{
			Value: User{UserId: "user-123", Email: "updated@example.com"},
		},
		nil,
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*User, error) {
		user := &User{UserId: "user-123", Email: "old@example.com"}
		return user.UpdateEmail(ctx, "updated@example.com")
	})

	s.True(s.env.IsWorkflowCompleted())
	s.NoError(s.env.GetWorkflowError())
	var result User
	s.NoError(s.env.GetWorkflowResult(&result))
	s.Equal("user-123", result.UserId)
	s.Equal("updated@example.com", result.Email)
}

func (s *UserServiceTestSuite) TestGetUserError() {
	s.env.OnNexusOperation(
		ServiceName,
		nexus.NewOperationReference[GetUserRequest, User](GetUserOp),
		GetUserRequest{UserId: "nonexistent"},
		mock.Anything,
	).Return(
		nil,
		nexus.NewOperationFailedErrorf("user not found"),
	)

	s.env.ExecuteWorkflow(func(ctx workflow.Context) (*User, error) {
		return GetUser(ctx, "nonexistent")
	})

	s.True(s.env.IsWorkflowCompleted())
	s.Error(s.env.GetWorkflowError())
}
