from __future__ import annotations

import datetime
from pathlib import Path
import uuid

from nexusrpc import Operation
from nexusrpc.handler import StartOperationContext, service_handler, sync_operation
import temporalio.common
from temporalio import workflow
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import UnsandboxedWorkflowRunner, Worker

import wit.type_roundtrip as type_roundtrip
import wit.type_roundtrip.models as type_roundtrip_models
import wit.type_roundtrip.services as type_roundtrip_services

OUTPUT_PATH = Path(__file__).resolve().parent.parent / "wit" / "type_roundtrip"
TASK_QUEUE = "demo-task-queue"

ACTIVITY_OPTIONS_OPERATION = type_roundtrip.__nexus_operation_registry__[
    ("TypeRoundtripService", "ActivityOptionsOperation")
]


@service_handler(service=type_roundtrip_services.TypeRoundtripService)
class TypeRoundtripServiceHandler:
    def __init__(self) -> None:
        self.calls: list[tuple[str, object]] = []

    @sync_operation
    async def activity_options_operation(
        self,
        _ctx: StartOperationContext,
        input: type_roundtrip_models.ActivityOptions,
    ) -> type_roundtrip_models.ActivityOptions:
        self.calls.append(("ActivityOptionsOperation", input))
        proto = input._temporal_to_wire()  # pyright: ignore[reportPrivateUsage]
        assert proto.HasField("retry_policy")
        assert proto.task_queue.name == TASK_QUEUE
        assert proto.schedule_to_close_timeout.seconds == 7
        assert proto.priority.priority_key == 4
        return input


@workflow.defn
class TypeRoundtripCallerWorkflow:
    @workflow.run
    async def run(self) -> tuple[str | None, int | None, int | None]:
        retry_policy = temporalio.common.RetryPolicy(maximum_attempts=3)
        activity_handle = await type_roundtrip.activity_options_operation(
            task_queue=TASK_QUEUE,
            schedule_to_close_timeout=datetime.timedelta(seconds=7),
            retry_policy=retry_policy,
            priority=temporalio.common.Priority(
                priority_key=4,
                fairness_key="tenant-a",
                fairness_weight=2.5,
            ),
        )
        activity_response = await activity_handle
        return (
            activity_response.task_queue,
            int(activity_response.schedule_to_close_timeout.total_seconds())
            if activity_response.schedule_to_close_timeout is not None
            else None,
            activity_response.priority.priority_key
            if activity_response.priority is not None
            else None,
        )


def retry_policy() -> temporalio.common.RetryPolicy:
    return temporalio.common.RetryPolicy(maximum_attempts=3)


def priority() -> temporalio.common.Priority:
    return temporalio.common.Priority(
        priority_key=4,
        fairness_key="tenant-a",
        fairness_weight=2.5,
    )


def test_generated_metadata() -> None:
    assert OUTPUT_PATH.exists(), f"expected generated package at {OUTPUT_PATH}"
    assert isinstance(ACTIVITY_OPTIONS_OPERATION, Operation)
    assert (
        type_roundtrip.__nexus_operation_registry__[
            ("TypeRoundtripService", "ActivityOptionsOperation")
        ]
        is ACTIVITY_OPTIONS_OPERATION
    )
    assert not hasattr(type_roundtrip, "TypeRoundtripService")
    assert not hasattr(type_roundtrip, "ActivityOptions")


def test_activity_options_round_trip() -> None:
    activity_options = type_roundtrip_models.ActivityOptions(
        retry_policy=retry_policy(),
        task_queue=TASK_QUEUE,
        schedule_to_close_timeout=datetime.timedelta(seconds=7),
        priority=priority(),
    )
    activity_proto = activity_options._temporal_to_wire()  # pyright: ignore[reportPrivateUsage]
    assert activity_proto.HasField("retry_policy")
    assert activity_proto.task_queue.name == TASK_QUEUE
    assert activity_proto.schedule_to_close_timeout.seconds == 7
    assert activity_proto.priority.priority_key == 4
    round_tripped_activity = type_roundtrip_models.ActivityOptions._temporal_from_wire(  # pyright: ignore[reportPrivateUsage]
        activity_proto
    )
    assert isinstance(
        round_tripped_activity.retry_policy, temporalio.common.RetryPolicy
    )
    assert round_tripped_activity.retry_policy.maximum_attempts == 3
    assert round_tripped_activity.task_queue == TASK_QUEUE
    assert round_tripped_activity.schedule_to_close_timeout == datetime.timedelta(
        seconds=7
    )
    assert round_tripped_activity.priority == priority()


async def test_operations_use_real_nexus_client(env: WorkflowEnvironment) -> None:
    task_queue = str(uuid.uuid4())
    service_handler = TypeRoundtripServiceHandler()

    async with Worker(
        env.client,
        task_queue=task_queue,
        workflows=[TypeRoundtripCallerWorkflow],
        nexus_service_handlers=[service_handler],
        workflow_runner=UnsandboxedWorkflowRunner(),
    ):
        endpoint = await env.create_nexus_endpoint("temporal-system", task_queue)
        try:
            result = await env.client.execute_workflow(
                TypeRoundtripCallerWorkflow.run,
                id=str(uuid.uuid4()),
                task_queue=task_queue,
            )
        finally:
            await env.delete_nexus_endpoint(endpoint)

    assert result == (TASK_QUEUE, 7, 4)
    assert len(service_handler.calls) == 1
    activity_operation, activity_request = service_handler.calls[0]
    assert activity_operation == "ActivityOptionsOperation"
    assert isinstance(activity_request, type_roundtrip_models.ActivityOptions)
