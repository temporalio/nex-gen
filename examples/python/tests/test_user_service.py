from __future__ import annotations

from pathlib import Path
import uuid

from nexusrpc import Operation
from nexusrpc.handler import StartOperationContext, service_handler, sync_operation
from temporalio import workflow
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import UnsandboxedWorkflowRunner, Worker

APP_ROOT = Path(__file__).resolve().parent
OUTPUT_PATH = APP_ROOT.parent / "user_service"

import user_service
import user_service.models
import user_service.service
from user_service._resources import User

GET_USER_OPERATION = user_service.__nexus_operation_registry__[
    ("UserService", "GetUser")
]
UPDATE_EMAIL_OPERATION = user_service.__nexus_operation_registry__[
    ("UserService", "UpdateEmail")
]


def user_resource(
    *,
    email: str,
) -> User:
    return User(
        user_id="user-123",
        email=email,
    )


@service_handler(service=user_service.service.UserService)
class UserServiceHandler:
    def __init__(self) -> None:
        self.calls: list[tuple[str, object]] = []

    @sync_operation
    async def get_user(
        self,
        _ctx: StartOperationContext,
        input: user_service.models.GetUserRequest,
    ) -> User:
        self.calls.append(("GetUser", input))
        assert input.user_id == "user-123"
        return user_resource(email="old@example.com")

    @sync_operation
    async def update_email(
        self,
        _ctx: StartOperationContext,
        input: user_service.models.UpdateEmailRequest,
    ) -> User:
        self.calls.append(("UpdateEmail", input))
        assert input.user_id == "user-123"
        assert input.email == "new@example.com"
        return user_resource(email=input.email)


@workflow.defn
class UserServiceCallerWorkflow:
    @workflow.run
    async def run(self) -> User:
        user = await user_service.get_user(user_id="user-123")
        return await user.update_email("new@example.com")


def test_generated_metadata() -> None:
    assert OUTPUT_PATH.exists(), f"expected generated package at {OUTPUT_PATH}"
    registry = user_service.__nexus_operation_registry__

    assert isinstance(GET_USER_OPERATION, Operation)
    assert GET_USER_OPERATION.name == "GetUser"
    assert registry[("UserService", "GetUser")] is GET_USER_OPERATION
    assert isinstance(UPDATE_EMAIL_OPERATION, Operation)
    assert UPDATE_EMAIL_OPERATION.name == "UpdateEmail"
    assert registry[("UserService", "UpdateEmail")] is UPDATE_EMAIL_OPERATION
    assert not hasattr(user_service, "UserService")
    assert not hasattr(user_service, "User")
    assert not hasattr(user_service.models.GetUserRequest, "to_proto")


async def test_get_user_returns_user_resource(env: WorkflowEnvironment) -> None:
    task_queue = str(uuid.uuid4())
    service_handler = UserServiceHandler()

    async with Worker(
        env.client,
        task_queue=task_queue,
        workflows=[UserServiceCallerWorkflow],
        nexus_service_handlers=[service_handler],
        workflow_runner=UnsandboxedWorkflowRunner(),
    ):
        endpoint = await env.create_nexus_endpoint("user-service", task_queue)
        try:
            user = await env.client.execute_workflow(
                UserServiceCallerWorkflow.run,
                id=str(uuid.uuid4()),
                task_queue=task_queue,
            )
        finally:
            await env.delete_nexus_endpoint(endpoint)

    assert isinstance(user, User)
    assert user.user_id == "user-123"
    assert user.email == "new@example.com"

    assert len(service_handler.calls) == 2
    get_user_operation, get_user_request = service_handler.calls[0]
    assert get_user_operation == "GetUser"
    assert isinstance(get_user_request, user_service.models.GetUserRequest)
    assert get_user_request.user_id == "user-123"

    update_operation, update_request = service_handler.calls[1]
    assert update_operation == "UpdateEmail"
    assert isinstance(update_request, user_service.models.UpdateEmailRequest)
    assert update_request.user_id == "user-123"
    assert update_request.email == "new@example.com"
