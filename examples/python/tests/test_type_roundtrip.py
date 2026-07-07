from __future__ import annotations

import datetime
from pathlib import Path
import uuid

from nexusrpc import Operation
from nexusrpc.handler import StartOperationContext, service_handler, sync_operation
import temporalio.api.activity.v1 as activity_v1
import temporalio.api.common.v1
import temporalio.common
from temporalio import workflow
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import UnsandboxedWorkflowRunner, Worker

import wit.type_roundtrip as type_roundtrip
import wit.type_roundtrip.models as type_roundtrip_models
import wit.type_roundtrip.services as type_roundtrip_services
import wit.type_roundtrip._support as type_roundtrip_support

OUTPUT_PATH = Path(__file__).resolve().parent.parent / "wit" / "type_roundtrip"
TASK_QUEUE = "demo-task-queue"

RETRY_POLICY_OPERATION = type_roundtrip.__nexus_operation_registry__[
    ("TypeRoundtripService", "RetryPolicyOperation")
]
ACTIVITY_OPTIONS_OPERATION = type_roundtrip.__nexus_operation_registry__[
    ("TypeRoundtripService", "ActivityOptionsOperation")
]


@service_handler(service=type_roundtrip_services.TypeRoundtripService)
class TypeRoundtripServiceHandler:
    def __init__(self) -> None:
        self.calls: list[tuple[str, object]] = []

    @sync_operation
    async def retry_policy_operation(
        self,
        _ctx: StartOperationContext,
        input: temporalio.api.common.v1.RetryPolicy,
    ) -> temporalio.api.common.v1.RetryPolicy:
        self.calls.append(("RetryPolicyOperation", input))
        response = temporalio.api.common.v1.RetryPolicy()
        response.CopyFrom(input)
        return response

    @sync_operation
    async def activity_options_operation(
        self,
        _ctx: StartOperationContext,
        input: activity_v1.ActivityOptions,
    ) -> activity_v1.ActivityOptions:
        self.calls.append(("ActivityOptionsOperation", input))
        assert input.HasField("retry_policy")
        assert input.task_queue.name == TASK_QUEUE
        assert input.schedule_to_close_timeout.seconds == 7
        assert input.priority.priority_key == 4
        response = activity_v1.ActivityOptions()
        response.CopyFrom(input)
        return response


@workflow.defn
class TypeRoundtripCallerWorkflow:
    @workflow.run
    async def run(self) -> tuple[int, str | None, int | None, int | None]:
        retry_policy = temporalio.common.RetryPolicy(maximum_attempts=3)
        retry_handle = await type_roundtrip.retry_policy_operation(retry_policy)
        retry_round_trip = type_roundtrip_support.retry_policy_from_proto(
            await retry_handle
        )

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
        activity_response = type_roundtrip_models.ActivityOptions.from_proto(
            await activity_handle
        )
        return (
            retry_round_trip.maximum_attempts,
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
    assert isinstance(RETRY_POLICY_OPERATION, Operation)
    assert isinstance(ACTIVITY_OPTIONS_OPERATION, Operation)
    assert (
        type_roundtrip.__nexus_operation_registry__[
            ("TypeRoundtripService", "RetryPolicyOperation")
        ]
        is RETRY_POLICY_OPERATION
    )
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
    activity_proto = activity_options.to_proto()
    assert activity_proto.HasField("retry_policy")
    assert activity_proto.task_queue.name == TASK_QUEUE
    assert activity_proto.schedule_to_close_timeout.seconds == 7
    assert activity_proto.priority.priority_key == 4
    round_tripped_activity = type_roundtrip_models.ActivityOptions.from_proto(
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

    assert result == (3, TASK_QUEUE, 7, 4)
    assert len(service_handler.calls) == 2
    retry_operation, retry_request = service_handler.calls[0]
    assert retry_operation == "RetryPolicyOperation"
    assert isinstance(retry_request, temporalio.api.common.v1.RetryPolicy)
    assert retry_request.maximum_attempts == 3
    activity_operation, activity_request = service_handler.calls[1]
    assert activity_operation == "ActivityOptionsOperation"
    assert isinstance(activity_request, activity_v1.ActivityOptions)
