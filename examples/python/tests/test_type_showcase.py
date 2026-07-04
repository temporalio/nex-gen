from __future__ import annotations

import base64
import json
from pathlib import Path
from typing import cast
import uuid

import nex_gen_runtime
from nexusrpc.handler import StartOperationContext, service_handler, sync_operation
from nexusrpc import Operation
from temporalio.api.common.v1 import Payloads
from temporalio import workflow
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import UnsandboxedWorkflowRunner, Worker

APP_ROOT = Path(__file__).resolve().parent
OUTPUT_PATH = APP_ROOT.parent / "wit" / "type_showcase"
WIRE_FIXTURE_DIR = APP_ROOT.parent.parent / "wire" / "type-showcase"
PYTHON_WIRE_FIXTURE = WIRE_FIXTURE_DIR / "set-profile-request.python.payloads"
TYPESCRIPT_WIRE_FIXTURE = WIRE_FIXTURE_DIR / "set-profile-request.typescript.payloads"
PYTHON_RECORD_SYNC_FIXTURE = WIRE_FIXTURE_DIR / "record-sync-request.python.payloads"
TYPESCRIPT_RECORD_SYNC_FIXTURE = (
    WIRE_FIXTURE_DIR / "record-sync-request.typescript.payloads"
)

import wit.type_showcase as type_showcase
import wit.type_showcase.models as type_showcase_models
import wit.type_showcase.service as type_showcase_service
from wit.type_showcase._resources import User

GET_USER_OPERATION = type_showcase.__nexus_operation_registry__[
    ("TypeShowcase", "GetUser")
]
UPDATE_EMAIL_OPERATION = type_showcase.__nexus_operation_registry__[
    ("TypeShowcase", "UpdateEmail")
]
RENAME_OPERATION = type_showcase.__nexus_operation_registry__[
    ("TypeShowcase", "Rename")
]
DEACTIVATE_OPERATION = type_showcase.__nexus_operation_registry__[
    ("TypeShowcase", "Deactivate")
]


def sample_set_profile_request() -> type_showcase_models.SetProfileRequest:
    return type_showcase_models.SetProfileRequest(
        user_id="user-123",
        profile=user_profile(),
    )


def user_profile() -> type_showcase_models.UserProfile:
    return type_showcase_models.UserProfile(
        capabilities=type_showcase_models.UserCapability.ReadProfile
        | type_showcase_models.UserCapability.UpdateEmail,
        notification_target=("email", "old@example.com"),
        sync_state=("ok", "synced"),
        address=type_showcase_models.PostalAddress(
            street="1 Main St",
            city="Portland",
            country="US",
            coordinates=(45.5152, -122.6784),
        ),
        metadata={"tier": "enterprise"},
        tags=["admin", "beta"],
    )


def sync_report() -> type_showcase_models.SyncReport:
    return type_showcase_models.SyncReport(
        route=[(45.5152, -122.6784), (47.6062, -122.3321)],
        attempts=[("ok", "synced"), ("err", "timeout")],
        region_status={
            "us-west": ("ok", "healthy"),
            "eu-central": ("err", "degraded"),
        },
    )


def user_resource(
    *,
    email: str,
    display_name: str,
) -> User:
    return User(
        user_id="user-123",
        email=email,
        display_name=display_name,
        status=type_showcase_models.UserStatus.Active,
        profile=user_profile(),
    )


def write_payloads(path: Path, payloads: Payloads) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    _ = path.write_text(
        base64.b64encode(payloads.SerializeToString(deterministic=True)).decode()
        + "\n",
        encoding="utf-8",
    )


def read_payloads(path: Path) -> Payloads:
    payloads = Payloads()
    _ = payloads.ParseFromString(base64.b64decode(path.read_text(encoding="utf-8")))
    return payloads


async def encode_request(request: type_showcase_models.SetProfileRequest) -> Payloads:
    return await nex_gen_runtime.nexus_data_converter.encode_wrapper([request])


async def decode_request(payloads: Payloads) -> type_showcase_models.SetProfileRequest:
    values = cast(
        list[object],
        await nex_gen_runtime.nexus_data_converter.decode_wrapper(
            payloads,
            [type_showcase_models.SetProfileRequest],
        ),
    )
    assert len(values) == 1
    value = values[0]
    assert isinstance(value, type_showcase_models.SetProfileRequest)
    return value


def payload_json(payloads: Payloads) -> dict[str, object]:
    assert len(payloads.payloads) == 1
    payload = payloads.payloads[0]
    assert payload.metadata["encoding"] == b"json/nexus"
    assert payload.metadata["nexusType"] == b"type-showcase.set-profile-request"
    value = cast(object, json.loads(payload.data))
    assert isinstance(value, dict)
    return cast(dict[str, object], value)


@service_handler(service=type_showcase_service.TypeShowcase)
class TypeShowcaseHandler:
    def __init__(self) -> None:
        self.calls: list[tuple[str, object]] = []

    @sync_operation
    async def get_user(
        self,
        _ctx: StartOperationContext,
        input: type_showcase_models.GetUserRequest,
    ) -> User:
        self.calls.append(("GetUser", input))
        assert input.user_id == "user-123"
        assert input.consistency_token == "read-123"
        return user_resource(email="old@example.com", display_name="Old Name")

    @sync_operation
    async def update_email(
        self,
        _ctx: StartOperationContext,
        input: type_showcase_models.UpdateEmailRequest,
    ) -> User:
        self.calls.append(("UpdateEmail", input))
        assert input.user_id == "user-123"
        assert input.email == "new@example.com"
        return user_resource(email=input.email, display_name="Old Name")

    @sync_operation
    async def rename(
        self,
        _ctx: StartOperationContext,
        input: type_showcase_models.RenameRequest,
    ) -> User:
        self.calls.append(("Rename", input))
        assert input.user_id == "user-123"
        assert input.display_name == "New Name"
        return user_resource(email="new@example.com", display_name=input.display_name)

    @sync_operation
    async def set_profile(
        self,
        _ctx: StartOperationContext,
        input: type_showcase_models.SetProfileRequest,
    ) -> User:
        self.calls.append(("SetProfile", input))
        return user_resource(email="old@example.com", display_name="Old Name")

    @sync_operation
    async def record_sync(
        self,
        _ctx: StartOperationContext,
        input: type_showcase_models.RecordSyncRequest,
    ) -> None:
        self.calls.append(("RecordSync", input))
        assert input.user_id == "user-123"
        assert input.report == sync_report()

    @sync_operation
    async def deactivate(
        self,
        _ctx: StartOperationContext,
        input: type_showcase_models.DeactivateRequest,
    ) -> None:
        self.calls.append(("Deactivate", input))
        assert input.user_id == "user-123"
        assert input.reason == "requested"


@workflow.defn
class TypeShowcaseCallerWorkflow:
    @workflow.run
    async def run(self) -> User:
        user = await type_showcase.get_user(
            user_id="user-123",
            consistency_token="read-123",
        )
        updated_user = await user.update_email("new@example.com")
        renamed_user = await updated_user.rename("New Name")
        await renamed_user.deactivate(reason="requested")
        record_sync_handle = await type_showcase.record_sync(
            user_id="user-123",
            report=sync_report(),
        )
        await record_sync_handle
        return renamed_user


def test_generated_metadata() -> None:
    assert OUTPUT_PATH.exists(), f"expected generated package at {OUTPUT_PATH}"
    registry = type_showcase.__nexus_operation_registry__

    assert isinstance(GET_USER_OPERATION, Operation)
    assert GET_USER_OPERATION.name == "GetUser"
    assert registry[("TypeShowcase", "GetUser")] is GET_USER_OPERATION
    assert isinstance(UPDATE_EMAIL_OPERATION, Operation)
    assert UPDATE_EMAIL_OPERATION.name == "UpdateEmail"
    assert registry[("TypeShowcase", "UpdateEmail")] is UPDATE_EMAIL_OPERATION
    assert isinstance(RENAME_OPERATION, Operation)
    assert RENAME_OPERATION.name == "Rename"
    assert registry[("TypeShowcase", "Rename")] is RENAME_OPERATION
    set_profile_operation = type_showcase.__nexus_operation_registry__[
        ("TypeShowcase", "SetProfile")
    ]
    assert isinstance(set_profile_operation, Operation)
    assert set_profile_operation.name == "SetProfile"
    assert registry[("TypeShowcase", "SetProfile")] is set_profile_operation
    assert isinstance(DEACTIVATE_OPERATION, Operation)
    assert DEACTIVATE_OPERATION.name == "Deactivate"
    assert registry[("TypeShowcase", "Deactivate")] is DEACTIVATE_OPERATION
    assert not hasattr(type_showcase, "TypeShowcase")
    assert not hasattr(type_showcase, "User")
    assert not hasattr(type_showcase_models, "DeactivateResponse")
    assert not hasattr(type_showcase_models.GetUserRequest, "to_proto")
    assert type_showcase_models.UserStatus.Active == 0
    assert type_showcase_models.UserCapability.ReadProfile == 1
    assert type_showcase_models.UserCapability.UpdateEmail == 2


def test_generated_wit_native_models_cover_common_wit_shapes() -> None:
    profile = user_profile()

    assert profile.notification_target == ("email", "old@example.com")
    assert profile.capabilities == (
        type_showcase_models.UserCapability.ReadProfile
        | type_showcase_models.UserCapability.UpdateEmail
    )
    assert profile.sync_state == ("ok", "synced")
    assert profile.address is not None
    assert profile.address.coordinates == (45.5152, -122.6784)
    assert profile.metadata == {"tier": "enterprise"}
    assert profile.tags == ["admin", "beta"]


async def test_type_showcase_request_wire_fixtures_are_cross_language_compatible() -> (
    None
):
    expected = sample_set_profile_request()
    python_payloads = await encode_request(expected)
    write_payloads(PYTHON_WIRE_FIXTURE, python_payloads)

    assert payload_json(python_payloads)["user-id"] == "user-123"
    assert await decode_request(read_payloads(PYTHON_WIRE_FIXTURE)) == expected
    assert await decode_request(read_payloads(TYPESCRIPT_WIRE_FIXTURE)) == expected


def sample_record_sync_request() -> type_showcase_models.RecordSyncRequest:
    return type_showcase_models.RecordSyncRequest(
        user_id="user-123",
        report=sync_report(),
    )


async def decode_record_sync_request(
    payloads: Payloads,
) -> type_showcase_models.RecordSyncRequest:
    values = cast(
        list[object],
        await nex_gen_runtime.nexus_data_converter.decode_wrapper(
            payloads,
            [type_showcase_models.RecordSyncRequest],
        ),
    )
    assert len(values) == 1
    value = values[0]
    assert isinstance(value, type_showcase_models.RecordSyncRequest)
    return value


async def test_record_sync_wire_fixtures_are_cross_language_compatible() -> None:
    """Containers of tuples and results -- including map keys containing
    dashes, which must be preserved verbatim -- round-trip across languages."""
    expected = sample_record_sync_request()
    python_payloads = await nex_gen_runtime.nexus_data_converter.encode_wrapper(
        [expected]
    )
    write_payloads(PYTHON_RECORD_SYNC_FIXTURE, python_payloads)

    assert (
        await decode_record_sync_request(read_payloads(PYTHON_RECORD_SYNC_FIXTURE))
        == expected
    )
    assert (
        await decode_record_sync_request(read_payloads(TYPESCRIPT_RECORD_SYNC_FIXTURE))
        == expected
    )


async def test_get_user_returns_wit_user_resource_through_real_nexus_client(
    env: WorkflowEnvironment,
) -> None:
    task_queue = str(uuid.uuid4())
    service_handler = TypeShowcaseHandler()

    async with Worker(
        env.client,
        task_queue=task_queue,
        workflows=[TypeShowcaseCallerWorkflow],
        nexus_service_handlers=[service_handler],
        workflow_runner=UnsandboxedWorkflowRunner(),
    ):
        endpoint = await env.create_nexus_endpoint("type-showcase", task_queue)
        try:
            user = await env.client.execute_workflow(
                TypeShowcaseCallerWorkflow.run,
                id=str(uuid.uuid4()),
                task_queue=task_queue,
            )
        finally:
            await env.delete_nexus_endpoint(endpoint)

    assert isinstance(user, User)
    assert user.user_id == "user-123"
    assert user.email == "new@example.com"
    assert user.display_name == "New Name"
    assert user.status is type_showcase_models.UserStatus.Active
    assert user.profile.notification_target == ("email", "old@example.com")
    assert user.profile.sync_state == ("ok", "synced")
    assert (
        user.profile.capabilities & type_showcase_models.UserCapability.ReadProfile
    ) == type_showcase_models.UserCapability.ReadProfile

    assert len(service_handler.calls) == 5
    get_user_operation, get_user_request = service_handler.calls[0]
    assert get_user_operation == "GetUser"
    assert isinstance(get_user_request, type_showcase_models.GetUserRequest)
    assert get_user_request.user_id == "user-123"
    assert get_user_request.consistency_token == "read-123"

    update_operation, update_request = service_handler.calls[1]
    assert update_operation == "UpdateEmail"
    assert isinstance(update_request, type_showcase_models.UpdateEmailRequest)
    assert update_request.user_id == "user-123"
    assert update_request.email == "new@example.com"

    rename_operation, rename_request = service_handler.calls[2]
    assert rename_operation == "Rename"
    assert isinstance(rename_request, type_showcase_models.RenameRequest)
    assert rename_request.user_id == "user-123"
    assert rename_request.display_name == "New Name"

    deactivate_operation, deactivate_request = service_handler.calls[3]
    assert deactivate_operation == "Deactivate"
    assert isinstance(deactivate_request, type_showcase_models.DeactivateRequest)
    assert deactivate_request.user_id == "user-123"
    assert deactivate_request.reason == "requested"

    # Containers of tuples and results round-trip through the json/nexus
    # wire format.
    record_sync_operation, record_sync_request = service_handler.calls[4]
    assert record_sync_operation == "RecordSync"
    assert isinstance(record_sync_request, type_showcase_models.RecordSyncRequest)
    assert record_sync_request.user_id == "user-123"
    assert record_sync_request.report == sync_report()
