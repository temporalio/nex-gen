from __future__ import annotations

from pathlib import Path
import typing
import uuid

from nexusrpc import Operation
from nexusrpc.handler import StartOperationContext, service_handler, sync_operation
import pytest
import temporalio.api.workflowservice.v1 as workflowservice_v1
from temporalio import workflow
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import UnsandboxedWorkflowRunner, Worker

APP_ROOT = Path(__file__).resolve().parent
OUTPUT_PATH = APP_ROOT.parent / "start_workflow"
TASK_QUEUE = "demo-task-queue"

import start_workflow
import start_workflow.models
import start_workflow.service
from start_workflow._resources import StartedWorkflow

START_WORKFLOW_OPERATION = start_workflow.__nexus_operation_registry__[
    ("WorkflowService", "StartWorkflow")
]
RESTART_WORKFLOW_OPERATION = start_workflow.__nexus_operation_registry__[
    ("WorkflowService", "RestartWorkflow")
]
CANCEL_WORKFLOW_OPERATION = start_workflow.__nexus_operation_registry__[
    ("WorkflowService", "CancelWorkflow")
]


@workflow.defn
class ExampleWorkflow:
    @workflow.run
    async def run(self, customer_id: str) -> str:
        return customer_id


@service_handler(service=start_workflow.service.WorkflowService)
class StartWorkflowServiceHandler:
    def __init__(self) -> None:
        self.calls: list[tuple[str, object]] = []

    @sync_operation
    async def start_workflow(
        self,
        _ctx: StartOperationContext,
        input: workflowservice_v1.StartWorkflowExecutionRequest,
    ) -> workflowservice_v1.StartWorkflowExecutionResponse:
        self.calls.append(("StartWorkflow", input))
        assert input.namespace == "default"
        assert input.workflow_id == "workflow-id"
        assert input.workflow_type.name == "ExampleWorkflow"
        assert input.task_queue.name == TASK_QUEUE
        assert len(input.input.payloads) == 1

        response = workflowservice_v1.StartWorkflowExecutionResponse()
        response.run_id = "run-123"
        response.started = True
        return response

    @sync_operation
    async def restart_workflow(
        self,
        _ctx: StartOperationContext,
        input: workflowservice_v1.StartWorkflowExecutionRequest,
    ) -> workflowservice_v1.StartWorkflowExecutionResponse:
        self.calls.append(("RestartWorkflow", input))
        assert input.namespace == "default"
        assert input.workflow_id == "workflow-id"
        assert input.workflow_type.name == "ExampleWorkflow"
        assert input.task_queue.name == TASK_QUEUE
        assert not input.HasField("input")

        response = workflowservice_v1.StartWorkflowExecutionResponse()
        response.run_id = "run-456"
        response.started = True
        return response

    @sync_operation
    async def cancel_workflow(
        self,
        _ctx: StartOperationContext,
        input: workflowservice_v1.RequestCancelWorkflowExecutionRequest,
    ) -> workflowservice_v1.RequestCancelWorkflowExecutionResponse:
        self.calls.append(("CancelWorkflow", input))
        assert input.namespace == "default"
        assert input.workflow_execution.workflow_id == "workflow-id"
        assert input.workflow_execution.run_id == "run-123"
        return workflowservice_v1.RequestCancelWorkflowExecutionResponse()


@workflow.defn
class StartWorkflowCallerWorkflow:
    @workflow.run
    async def run(self) -> tuple[str, str, str | None, str | None]:
        handle = await start_workflow.start_workflow(
            ExampleWorkflow.run,
            "customer-123",
            workflow_id="workflow-id",
            task_queue=TASK_QUEUE,
        )
        await handle.cancel()
        restarted_handle = await handle.restart_workflow(
            workflow=ExampleWorkflow.run,
            task_queue=TASK_QUEUE,
        )
        try:
            _ = await restarted_handle.get_result()
        except NotImplementedError:
            pass
        else:
            raise AssertionError(
                "started-workflow.get_result should not be implemented"
            )
        return (
            handle.namespace,
            handle.workflow_id,
            handle.run_id,
            restarted_handle.run_id,
        )


def test_generated_metadata() -> None:
    assert OUTPUT_PATH.exists(), f"expected generated package at {OUTPUT_PATH}"
    start_operation = START_WORKFLOW_OPERATION
    restart_operation = RESTART_WORKFLOW_OPERATION
    cancel_operation = CANCEL_WORKFLOW_OPERATION
    registry = start_workflow.__nexus_operation_registry__

    assert isinstance(start_operation, Operation)
    assert start_operation.name == "StartWorkflow"
    assert registry[("WorkflowService", "StartWorkflow")] is start_operation
    assert isinstance(cancel_operation, Operation)
    assert cancel_operation.name == "CancelWorkflow"
    assert registry[("WorkflowService", "CancelWorkflow")] is cancel_operation
    assert isinstance(restart_operation, Operation)
    assert restart_operation.name == "RestartWorkflow"
    assert registry[("WorkflowService", "RestartWorkflow")] is restart_operation
    assert not hasattr(start_workflow, "WorkflowService")
    assert not hasattr(start_workflow, "StartWorkflowExecutionRequest")
    assert not hasattr(start_workflow, "StartedWorkflow")


def test_workflow_execution_serializes() -> None:
    request = start_workflow.models.WorkflowExecution(workflow_id="workflow-id")
    proto = request.to_proto()

    assert proto.workflow_id == "workflow-id"
    assert proto.run_id == ""


async def test_start_workflow_returns_wrapper_handle(
    env: WorkflowEnvironment,
) -> None:
    task_queue = str(uuid.uuid4())
    service_handler = StartWorkflowServiceHandler()

    async with Worker(
        env.client,
        task_queue=task_queue,
        workflows=[StartWorkflowCallerWorkflow],
        nexus_service_handlers=[service_handler],
        workflow_runner=UnsandboxedWorkflowRunner(),
    ):
        endpoint = await env.create_nexus_endpoint("temporal-system", task_queue)
        try:
            result = await env.client.execute_workflow(
                StartWorkflowCallerWorkflow.run,
                id=str(uuid.uuid4()),
                task_queue=task_queue,
            )
        finally:
            await env.delete_nexus_endpoint(endpoint)

    assert result == ("default", "workflow-id", "run-123", "run-456")
    assert len(service_handler.calls) == 3
    start_operation, start_request = service_handler.calls[0]
    assert start_operation == "StartWorkflow"
    assert isinstance(start_request, workflowservice_v1.StartWorkflowExecutionRequest)
    cancel_operation, cancel_request = service_handler.calls[1]
    assert cancel_operation == "CancelWorkflow"
    assert isinstance(
        cancel_request,
        workflowservice_v1.RequestCancelWorkflowExecutionRequest,
    )
    restart_operation, restart_request = service_handler.calls[2]
    assert restart_operation == "RestartWorkflow"
    assert isinstance(restart_request, workflowservice_v1.StartWorkflowExecutionRequest)

    result_error: pytest.ExceptionInfo[NotImplementedError]
    with pytest.raises(NotImplementedError) as result_error:
        _ = await StartedWorkflow(
            namespace="default",
            workflow_id="workflow-id",
            run_id="run-456",
        ).get_result()
    assert (
        result_error.value.args[0]
        == "started-workflow.get_result is not yet implemented"
    )


if typing.TYPE_CHECKING:
    _ = start_workflow.start_workflow(
        ExampleWorkflow.run,
        "customer-123",
        workflow_id="typed-positional-workflow-input",
        task_queue=TASK_QUEUE,
    )

    _ = start_workflow.start_workflow(
        workflow=ExampleWorkflow.run,
        args=["customer-123"],
        workflow_id="typed-list-workflow-input",
        task_queue=TASK_QUEUE,
    )

    start_workflow.start_workflow(  # pyright: ignore[reportCallIssue]
        ExampleWorkflow.run,
        "customer-123",
        args=["customer-456"],
        workflow_id="conflicting-typed-workflow-input",
        task_queue=TASK_QUEUE,
    )

    _ = start_workflow.start_workflow(
        workflow=ExampleWorkflow.run,
        args=["customer-123"],
        workflow_id="typed-list-workflow-input-fallback",
        task_queue=TASK_QUEUE,
    )

    start_workflow.start_workflow(  # pyright: ignore[reportCallIssue]
        "ExampleWorkflow",
        "customer-123",
        args=["customer-456"],
        workflow_id="conflicting-string-workflow-input",
        task_queue=TASK_QUEUE,
    )

    start_workflow.start_workflow(  # pyright: ignore[reportCallIssue]
        workflow=ExampleWorkflow.run,
        args="customer-123",  # pyright: ignore[reportArgumentType]
        workflow_id="typed-scalar-keyword-workflow-input",
        task_queue=TASK_QUEUE,
    )
