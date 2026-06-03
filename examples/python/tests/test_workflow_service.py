from __future__ import annotations

from collections.abc import Awaitable, Callable, Sequence
import dataclasses
import datetime
from pathlib import Path
import typing
import uuid

from nexusrpc import Operation
from nexusrpc.handler import StartOperationContext, service_handler, sync_operation
import temporalio.api.common.v1 as common_v1
import temporalio.api.workflowservice.v1 as workflowservice_v1
import temporalio.common
from temporalio import workflow
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import UnsandboxedWorkflowRunner, Worker
from typing_extensions import assert_type
import pytest
import workflow_service
import workflow_service.models
import workflow_service.service

APP_ROOT = Path(__file__).resolve().parent
OUTPUT_PATH = APP_ROOT.parent / "workflow_service"

SIGNAL_WITH_START_OPERATION = workflow_service.__nexus_operation_registry__[
    (
        "temporal.api.workflowservice.v1.WorkflowService",
        "SignalWithStartWorkflowExecution",
    )
]

TASK_QUEUE = "demo-task-queue"
REQUEST_ID = "example-request"
CRON_SCHEDULE = ""

REQUEST_WORKFLOW_ID = "workflow-request"
ARGS_WORKFLOW_ID = "workflow-args"
POSITIONAL_WORKFLOW_ID = "workflow-positional"
TUPLE_ONE_WORKFLOW_ID = "workflow-tuple-one"
MINIMAL_WORKFLOW_ID = "workflow-minimal"
HIGH_ARITY_WORKFLOW_ID = "workflow-high-arity"
LIST_SIGNAL_SEPARATE_WORKFLOW_ID = "workflow-list-signal-separate"
LIST_SIGNAL_WRAPPED_WORKFLOW_ID = "workflow-list-signal-wrapped"

FULL_WORKFLOW_INPUT = [7, "nexus"]
ARGS_SIGNAL_INPUT = ["wake-up"]
LIST_SIGNAL_INPUT = ["one", "two"]
HIGH_ARITY_SIGNAL_INPUT = [
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
]


@workflow.defn
class ExampleWorkflow:
    @workflow.run
    async def run(self, attempt: int, name: str) -> str:
        return f"{attempt}:{name}"

    @workflow.signal
    def wake_up(self, reason: str) -> None:
        _ = reason

    @workflow.signal
    def wake_up_list(self, values: list[str]) -> None:
        _ = values

    @workflow.signal
    def wake_up_many(
        self,
        first: str,
        second: str,
        third: str,
        fourth: str,
        fifth: str,
        sixth: str,
        seventh: str,
    ) -> None:
        _ = (first, second, third, fourth, fifth, sixth, seventh)


@workflow.defn
class SingleArgWorkflow:
    @workflow.run
    async def run(self, value: str) -> str:
        return value


@dataclasses.dataclass(frozen=True)
class ExampleData:
    retry_policy: temporalio.common.RetryPolicy
    priority: temporalio.common.Priority
    versioning_override: temporalio.common.VersioningOverride
    typed_search_attributes: temporalio.common.TypedSearchAttributes


def expected_run_id(workflow_id: str) -> str:
    return f"run-for-{workflow_id}"


def assert_payload_count(
    payloads: common_v1.Payloads,
    expected_count: int,
) -> None:
    assert len(payloads.payloads) == expected_count


def assert_common_signal_request(
    request: workflowservice_v1.SignalWithStartWorkflowExecutionRequest,
    *,
    workflow_id: str,
    signal_name: str,
    workflow_type: str = "ExampleWorkflow",
) -> None:
    assert request.namespace == "default"
    assert request.workflow_id == workflow_id
    assert request.signal_name == signal_name
    assert request.workflow_type.name == workflow_type
    assert request.task_queue.name == TASK_QUEUE


def assert_full_signal_request(
    request: workflowservice_v1.SignalWithStartWorkflowExecutionRequest,
    *,
    workflow_id: str,
    signal_name: str,
    signal_input: Sequence[object] | None,
) -> None:
    assert_common_signal_request(
        request,
        workflow_id=workflow_id,
        signal_name=signal_name,
    )
    assert request.HasField("input")
    assert_payload_count(request.input, len(FULL_WORKFLOW_INPUT))
    if signal_input is None:
        assert not request.HasField("signal_input")
    else:
        assert request.HasField("signal_input")
        assert_payload_count(request.signal_input, len(signal_input))
    assert request.workflow_execution_timeout.seconds == 30
    assert request.retry_policy.maximum_attempts == 3
    assert request.workflow_id_reuse_policy == int(
        temporalio.common.WorkflowIDReusePolicy.ALLOW_DUPLICATE_FAILED_ONLY
    )
    assert request.workflow_id_conflict_policy == int(
        temporalio.common.WorkflowIDConflictPolicy.TERMINATE_EXISTING
    )
    assert request.priority.priority_key == 4
    assert request.priority.fairness_key == "tenant-a"
    assert request.priority.fairness_weight == 2.5
    assert "CustomKeywordField" in request.search_attributes.indexed_fields
    assert request.user_metadata.summary.data
    assert request.user_metadata.details.data
    assert request.versioning_override.HasField("pinned")
    assert request.versioning_override.pinned.version.deployment_name == "payments"
    assert request.versioning_override.pinned.version.build_id == "build-42"


def assert_minimal_signal_request(
    request: workflowservice_v1.SignalWithStartWorkflowExecutionRequest,
) -> None:
    assert_common_signal_request(
        request,
        workflow_id=MINIMAL_WORKFLOW_ID,
        signal_name="wake_up",
    )
    assert not request.HasField("input")
    assert not request.HasField("workflow_execution_timeout")
    assert not request.HasField("retry_policy")
    assert request.workflow_id_reuse_policy == int(
        temporalio.common.WorkflowIDReusePolicy.ALLOW_DUPLICATE
    )
    assert request.workflow_id_conflict_policy == int(
        temporalio.common.WorkflowIDConflictPolicy.UNSPECIFIED
    )
    assert not request.HasField("priority")
    assert len(request.memo.fields) == 0
    assert len(request.search_attributes.indexed_fields) == 0
    assert not request.HasField("user_metadata")
    assert not request.HasField("versioning_override")
    assert not request.HasField("signal_input")


def assert_single_arg_signal_request(
    request: workflowservice_v1.SignalWithStartWorkflowExecutionRequest,
    *,
    workflow_id: str,
) -> None:
    assert_common_signal_request(
        request,
        workflow_id=workflow_id,
        signal_name="wake_up",
        workflow_type="SingleArgWorkflow",
    )
    assert request.HasField("input")
    assert_payload_count(request.input, 1)
    assert not request.HasField("signal_input")


def build_example_data() -> ExampleData:
    retry_policy = temporalio.common.RetryPolicy(maximum_attempts=3)
    priority = temporalio.common.Priority(
        priority_key=4,
        fairness_key="tenant-a",
        fairness_weight=2.5,
    )
    versioning_override = temporalio.common.PinnedVersioningOverride(
        temporalio.common.WorkerDeploymentVersion(
            deployment_name="payments",
            build_id="build-42",
        )
    )
    search_key = temporalio.common.SearchAttributeKey.for_keyword("CustomKeywordField")
    typed_search_attributes = temporalio.common.TypedSearchAttributes(
        [temporalio.common.SearchAttributePair(search_key, "sample-value")]
    )
    return ExampleData(
        retry_policy=retry_policy,
        priority=priority,
        versioning_override=versioning_override,
        typed_search_attributes=typed_search_attributes,
    )


def build_full_signal_request(
    example_data: ExampleData,
    *,
    workflow_id: str,
    signal: str | Callable[..., None | Awaitable[None]],
    signal_args: list[typing.Any] | None = None,
) -> workflow_service.models.SignalWithStartWorkflowRequest:
    return workflow_service.models.SignalWithStartWorkflowRequest(
        workflow=ExampleWorkflow.run,
        id=workflow_id,
        task_queue=TASK_QUEUE,
        signal=signal,
        args=FULL_WORKFLOW_INPUT,
        execution_timeout=datetime.timedelta(seconds=30),
        request_id=REQUEST_ID,
        retry_policy=example_data.retry_policy,
        id_reuse_policy=temporalio.common.WorkflowIDReusePolicy.ALLOW_DUPLICATE_FAILED_ONLY,
        id_conflict_policy=temporalio.common.WorkflowIDConflictPolicy.TERMINATE_EXISTING,
        signal_args=signal_args,
        cron_schedule=CRON_SCHEDULE,
        search_attributes=example_data.typed_search_attributes,
        user_metadata=workflow_service.models.UserMetadata(
            static_summary="Nightly sync",
            static_details="Processes 42 records",
        ),
        priority=example_data.priority,
        versioning_override=example_data.versioning_override,
    )


@service_handler(service=workflow_service.service.WorkflowService)
class WorkflowServiceHandler:
    def __init__(self) -> None:
        self.calls: list[
            workflowservice_v1.SignalWithStartWorkflowExecutionRequest
        ] = []

    @sync_operation
    async def signal_with_start_workflow(
        self,
        _ctx: StartOperationContext,
        input: workflowservice_v1.SignalWithStartWorkflowExecutionRequest,
    ) -> workflowservice_v1.SignalWithStartWorkflowExecutionResponse:
        self.calls.append(input)
        response = workflowservice_v1.SignalWithStartWorkflowExecutionResponse()
        response.run_id = expected_run_id(input.workflow_id)
        response.started = True
        return response


@workflow.defn
class WorkflowServiceCallerWorkflow:
    @workflow.run
    async def run(self) -> list[tuple[str, str | None]]:
        example_data = build_example_data()
        request = build_full_signal_request(
            example_data,
            workflow_id=REQUEST_WORKFLOW_ID,
            signal="wake_up",
        )
        request_proto = request.to_proto()
        round_tripped_user_metadata = workflow_service.models.UserMetadata.from_proto(
            request_proto.user_metadata
        )
        assert round_tripped_user_metadata.static_summary
        assert round_tripped_user_metadata.static_details

        request_handle = await workflow_service.signal_with_start_workflow(
            workflow="ExampleWorkflow",
            id=ARGS_WORKFLOW_ID,
            task_queue=TASK_QUEUE,
            signal="wake_up",
            args=list(FULL_WORKFLOW_INPUT),
            signal_args=["wake-up"],
            execution_timeout=datetime.timedelta(seconds=30),
            request_id=REQUEST_ID,
            retry_policy=example_data.retry_policy,
            id_reuse_policy=temporalio.common.WorkflowIDReusePolicy.ALLOW_DUPLICATE_FAILED_ONLY,
            id_conflict_policy=temporalio.common.WorkflowIDConflictPolicy.TERMINATE_EXISTING,
            cron_schedule=CRON_SCHEDULE,
            search_attributes=example_data.typed_search_attributes,
            static_summary="Nightly sync",
            static_details="Processes 42 records",
            priority=example_data.priority,
            versioning_override=example_data.versioning_override,
        )
        positional_handle = await workflow_service.signal_with_start_workflow(
            SingleArgWorkflow.run,
            "positional",
            id=POSITIONAL_WORKFLOW_ID,
            task_queue=TASK_QUEUE,
            request_id=REQUEST_ID,
            signal="wake_up",
            cron_schedule=CRON_SCHEDULE,
        )
        tuple_one_handle = await workflow_service.signal_with_start_workflow(
            SingleArgWorkflow.run,
            args=["tuple-one"],
            id=TUPLE_ONE_WORKFLOW_ID,
            task_queue=TASK_QUEUE,
            request_id=REQUEST_ID,
            signal="wake_up",
            cron_schedule=CRON_SCHEDULE,
        )
        minimal_handle = await workflow_service.signal_with_start_workflow(
            workflow="ExampleWorkflow",
            id=MINIMAL_WORKFLOW_ID,
            task_queue=TASK_QUEUE,
            request_id=REQUEST_ID,
            signal="wake_up",
            cron_schedule=CRON_SCHEDULE,
        )
        high_arity_request = build_full_signal_request(
            example_data,
            workflow_id=HIGH_ARITY_WORKFLOW_ID,
            signal=ExampleWorkflow.wake_up_many,
            signal_args=HIGH_ARITY_SIGNAL_INPUT,
        )
        high_arity_proto = high_arity_request.to_proto()
        assert_full_signal_request(
            high_arity_proto,
            workflow_id=HIGH_ARITY_WORKFLOW_ID,
            signal_name="wake_up_many",
            signal_input=HIGH_ARITY_SIGNAL_INPUT,
        )
        list_signal_separate_handle = await workflow_service.signal_with_start_workflow(
            workflow="ExampleWorkflow",
            id=LIST_SIGNAL_SEPARATE_WORKFLOW_ID,
            task_queue=TASK_QUEUE,
            request_id=REQUEST_ID,
            signal=ExampleWorkflow.wake_up_list,
            signal_args=LIST_SIGNAL_INPUT,
            cron_schedule=CRON_SCHEDULE,
        )
        list_signal_wrapped_handle = await workflow_service.signal_with_start_workflow(
            workflow="ExampleWorkflow",
            id=LIST_SIGNAL_WRAPPED_WORKFLOW_ID,
            task_queue=TASK_QUEUE,
            request_id=REQUEST_ID,
            signal=ExampleWorkflow.wake_up_list,
            signal_args=[LIST_SIGNAL_INPUT],
            cron_schedule=CRON_SCHEDULE,
        )
        return [
            (request_handle.id, request_handle.run_id),
            (positional_handle.id, positional_handle.run_id),
            (tuple_one_handle.id, tuple_one_handle.run_id),
            (minimal_handle.id, minimal_handle.run_id),
            (list_signal_separate_handle.id, list_signal_separate_handle.run_id),
            (list_signal_wrapped_handle.id, list_signal_wrapped_handle.run_id),
        ]


def assert_handle_matches(
    handle: workflow.ExternalWorkflowHandle[typing.Any],
    workflow_id: str,
) -> None:
    _ = assert_type(
        handle,
        workflow.ExternalWorkflowHandle[typing.Any],
    )
    assert handle.id == workflow_id
    assert handle.run_id == expected_run_id(workflow_id)


def test_generated_metadata() -> None:
    assert OUTPUT_PATH.exists(), f"expected generated package at {OUTPUT_PATH}"
    signal_operation = SIGNAL_WITH_START_OPERATION
    registry = workflow_service.__nexus_operation_registry__

    assert isinstance(signal_operation, Operation)
    assert (
        registry[
            (
                "temporal.api.workflowservice.v1.WorkflowService",
                "SignalWithStartWorkflowExecution",
            )
        ]
        is signal_operation
    )
    assert not hasattr(workflow_service, "WorkflowService")
    assert not hasattr(workflow_service, "SignalWithStartWorkflowRequest")
    assert not hasattr(workflow_service, "UserMetadata")
    assert not hasattr(workflow_service, "workflow")


async def test_signal_with_start_uses_real_nexus_client(
    env: WorkflowEnvironment,
) -> None:
    task_queue = str(uuid.uuid4())
    service_handler = WorkflowServiceHandler()

    async with Worker(
        env.client,
        task_queue=task_queue,
        workflows=[WorkflowServiceCallerWorkflow, SingleArgWorkflow],
        nexus_service_handlers=[service_handler],
        workflow_runner=UnsandboxedWorkflowRunner(),
    ):
        endpoint = await env.create_nexus_endpoint("temporal-system", task_queue)
        try:
            handles = await env.client.execute_workflow(
                WorkflowServiceCallerWorkflow.run,
                id=str(uuid.uuid4()),
                task_queue=task_queue,
            )
        finally:
            await env.delete_nexus_endpoint(endpoint)

    assert handles == [
        (ARGS_WORKFLOW_ID, expected_run_id(ARGS_WORKFLOW_ID)),
        (POSITIONAL_WORKFLOW_ID, expected_run_id(POSITIONAL_WORKFLOW_ID)),
        (TUPLE_ONE_WORKFLOW_ID, expected_run_id(TUPLE_ONE_WORKFLOW_ID)),
        (MINIMAL_WORKFLOW_ID, expected_run_id(MINIMAL_WORKFLOW_ID)),
        (
            LIST_SIGNAL_SEPARATE_WORKFLOW_ID,
            expected_run_id(LIST_SIGNAL_SEPARATE_WORKFLOW_ID),
        ),
        (
            LIST_SIGNAL_WRAPPED_WORKFLOW_ID,
            expected_run_id(LIST_SIGNAL_WRAPPED_WORKFLOW_ID),
        ),
    ]
    assert len(service_handler.calls) == 6
    assert_full_signal_request(
        service_handler.calls[0],
        workflow_id=ARGS_WORKFLOW_ID,
        signal_name="wake_up",
        signal_input=ARGS_SIGNAL_INPUT,
    )
    assert_single_arg_signal_request(
        service_handler.calls[1],
        workflow_id=POSITIONAL_WORKFLOW_ID,
    )
    assert_single_arg_signal_request(
        service_handler.calls[2],
        workflow_id=TUPLE_ONE_WORKFLOW_ID,
    )
    assert_minimal_signal_request(service_handler.calls[3])
    assert_common_signal_request(
        service_handler.calls[4],
        workflow_id=LIST_SIGNAL_SEPARATE_WORKFLOW_ID,
        signal_name="wake_up_list",
    )
    assert_payload_count(service_handler.calls[4].signal_input, len(LIST_SIGNAL_INPUT))
    assert_common_signal_request(
        service_handler.calls[5],
        workflow_id=LIST_SIGNAL_WRAPPED_WORKFLOW_ID,
        signal_name="wake_up_list",
    )
    assert_payload_count(service_handler.calls[5].signal_input, 1)


async def test_signal_with_start_rejects_positional_args_and_args() -> None:
    with pytest.raises(
        TypeError, match="cannot specify both positional arguments and args"
    ):
        _ = await workflow_service.signal_with_start_workflow(
            "SingleArgWorkflow",
            "positional",
            args=["tuple-one"],
            id="workflow-conflicting-args",
            task_queue=TASK_QUEUE,
            request_id=REQUEST_ID,
            signal="wake_up",
            cron_schedule=CRON_SCHEDULE,
        )


if typing.TYPE_CHECKING:

    async def _typecheck_signal_with_start_return_types() -> None:
        positional_handle = await workflow_service.signal_with_start_workflow(
            SingleArgWorkflow.run,
            "positional",
            id="typed-return-positional-workflow-input",
            task_queue=TASK_QUEUE,
            request_id=REQUEST_ID,
            signal="wake_up",
            cron_schedule=CRON_SCHEDULE,
        )
        _ = assert_type(positional_handle, workflow.ExternalWorkflowHandle[str])

        list_args_handle = await workflow_service.signal_with_start_workflow(
            workflow=ExampleWorkflow.run,
            args=[7, "nexus"],
            id="typed-return-list-workflow-input",
            task_queue=TASK_QUEUE,
            request_id=REQUEST_ID,
            signal="wake_up",
            cron_schedule=CRON_SCHEDULE,
        )
        _ = assert_type(list_args_handle, workflow.ExternalWorkflowHandle[str])

        string_workflow_handle = await workflow_service.signal_with_start_workflow(
            workflow="ExampleWorkflow",
            id="string-workflow-return",
            task_queue=TASK_QUEUE,
            request_id=REQUEST_ID,
            signal="wake_up",
            cron_schedule=CRON_SCHEDULE,
        )
        _ = assert_type(
            string_workflow_handle,
            workflow.ExternalWorkflowHandle[typing.Any],
        )

    _ = _typecheck_signal_with_start_return_types

    _ = workflow_service.signal_with_start_workflow(
        SingleArgWorkflow.run,
        "positional",
        id="single-positional-workflow-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal="wake_up",
        cron_schedule=CRON_SCHEDULE,
    )

    _ = workflow_service.signal_with_start_workflow(
        workflow=SingleArgWorkflow.run,
        args=["tuple-one"],
        id="single-list-workflow-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal="wake_up",
        cron_schedule=CRON_SCHEDULE,
    )

    _ = workflow_service.signal_with_start_workflow(
        workflow=ExampleWorkflow.run,
        args=FULL_WORKFLOW_INPUT,
        id="multi-list-workflow-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal="wake_up",
        cron_schedule=CRON_SCHEDULE,
    )

    _ = workflow_service.signal_with_start_workflow(
        workflow="ExampleWorkflow",
        id="typed-signal-scalar-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal=ExampleWorkflow.wake_up,
        signal_args="wake-up",
        cron_schedule=CRON_SCHEDULE,
    )

    _ = workflow_service.signal_with_start_workflow(
        workflow="ExampleWorkflow",
        id="typed-signal-list-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal=ExampleWorkflow.wake_up,
        signal_args=["wake-up"],
        cron_schedule=CRON_SCHEDULE,
    )

    _ = workflow_service.signal_with_start_workflow(
        workflow="ExampleWorkflow",
        id="high-arity-signal-tuple-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal="wake_up_many",
        signal_args=HIGH_ARITY_SIGNAL_INPUT,
        cron_schedule=CRON_SCHEDULE,
    )

    _ = workflow_service.signal_with_start_workflow(
        workflow=ExampleWorkflow.run,
        args=[7, "nexus"],
        id="typed-list-workflow-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal="wake_up",
        cron_schedule=CRON_SCHEDULE,
    )

    workflow_service.signal_with_start_workflow(  # pyright: ignore[reportCallIssue]
        SingleArgWorkflow.run,  # pyright: ignore[reportArgumentType]
        "positional",
        args=["tuple-one"],
        id="conflicting-typed-callable-workflow-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal="wake_up",
        cron_schedule=CRON_SCHEDULE,
    )

    workflow_service.signal_with_start_workflow(  # pyright: ignore[reportCallIssue]
        workflow=SingleArgWorkflow.run,
        args="scalar",  # pyright: ignore[reportArgumentType]
        id="typed-scalar-keyword-workflow-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal="wake_up",
        cron_schedule=CRON_SCHEDULE,
    )

    workflow_service.signal_with_start_workflow(  # pyright: ignore[reportArgumentType, reportCallIssue]
        SingleArgWorkflow.run,
        "positional",
        "extra",
        id="too-many-positional-workflow-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal="wake_up",
        cron_schedule=CRON_SCHEDULE,
    )

    workflow_service.signal_with_start_workflow(  # pyright: ignore[reportArgumentType, reportCallIssue]
        workflow=ExampleWorkflow.run,
        id="missing-workflow-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal="wake_up",
        cron_schedule=CRON_SCHEDULE,
    )

    workflow_service.signal_with_start_workflow(  # pyright: ignore[reportArgumentType, reportCallIssue]
        ExampleWorkflow.run,
        3,
        4,
        id="bad-workflow-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal="wake_up",
        cron_schedule=CRON_SCHEDULE,
    )

    workflow_service.signal_with_start_workflow(  # pyright: ignore[reportCallIssue]
        workflow="ExampleWorkflow",
        id="missing-signal-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal=ExampleWorkflow.wake_up,  # pyright: ignore[reportArgumentType]
        cron_schedule=CRON_SCHEDULE,
    )

    workflow_service.signal_with_start_workflow(  # pyright: ignore[reportCallIssue]
        workflow="ExampleWorkflow",
        id="bad-signal-input",
        task_queue=TASK_QUEUE,
        request_id=REQUEST_ID,
        signal=ExampleWorkflow.wake_up,
        signal_args=7,  # pyright: ignore[reportArgumentType]
        cron_schedule=CRON_SCHEDULE,
    )
