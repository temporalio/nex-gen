"""Runtime test driving the generated KB Nexus service definition end-to-end.

Unlike ``test_kb.py`` (which round-trips wire fixtures through the data
converter), this exercises the generated ``KnowledgeBaseService`` *service and
operation definitions* over a real Temporal + Nexus endpoint. The caller
workflow uses the Temporal SDK's built-in Nexus client directly — there is no
generated API client — and references the generated operation definitions for
end-to-end type safety.
"""

from __future__ import annotations

import json
import shutil
from pathlib import Path
import typing
import uuid

from nexusrpc.handler import StartOperationContext, service_handler, sync_operation
from temporalio import workflow
from temporalio.contrib.pydantic import pydantic_data_converter
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import UnsandboxedWorkflowRunner, Worker

from kb import Block, Category, Page
from kb.kb.models import GetCategoryTreeInput, GetPageInput, PutBlockOutput
from kb.kb.services import KnowledgeBaseService

WIRE_FIXTURE_DIR = Path(__file__).resolve().parents[2] / "wire" / "json_schema" / "kb"
ENDPOINT = "knowledge-base"


def load_fixture(name: str) -> typing.Any:
    return json.loads((WIRE_FIXTURE_DIR / name).read_text(encoding="utf-8"))


@service_handler(service=KnowledgeBaseService)
class KnowledgeBaseServiceHandler:
    def __init__(self) -> None:
        self.calls: list[tuple[str, object]] = []

    @sync_operation
    async def get_page(self, _ctx: StartOperationContext, input: GetPageInput) -> Page:
        self.calls.append(("GetPage", input))
        assert input.page_id == "page-1"
        return Page.model_validate(load_fixture("page.json"))

    @sync_operation
    async def put_block(
        self, _ctx: StartOperationContext, input: Block
    ) -> PutBlockOutput:
        self.calls.append(("PutBlock", input))
        assert input.block_id == "block-1"
        assert input.style is not None
        assert input.style.bold is True
        return PutBlockOutput.model_validate(load_fixture("put-block-output.json"))

    @sync_operation
    async def get_category_tree(
        self, _ctx: StartOperationContext, input: GetCategoryTreeInput
    ) -> Category:
        self.calls.append(("GetCategoryTree", input))
        assert input.root_id == "root"
        return Category.model_validate(load_fixture("category-tree.json"))


@workflow.defn
class KnowledgeBaseCallerWorkflow:
    @workflow.run
    async def run(self) -> dict[str, object]:
        client = workflow.create_nexus_client(
            service=KnowledgeBaseService, endpoint=ENDPOINT
        )

        page = await client.execute_operation(
            KnowledgeBaseService.get_page, GetPageInput(pageId="page-1")
        )
        block = page.blocks[0] if page.blocks is not None else None
        if block is None:
            raise RuntimeError("expected page block")

        put_block_output = await client.execute_operation(
            KnowledgeBaseService.put_block, block
        )

        category = await client.execute_operation(
            KnowledgeBaseService.get_category_tree,
            GetCategoryTreeInput(rootId="root"),
        )

        return {
            "blockId": put_block_output.block_id,
            "categoryChildId": category.children[0].id
            if category.children is not None
            else None,
            "pageId": page.page_id,
            "revision": put_block_output.revision,
        }


async def test_kb_operations_use_real_nexus_client() -> None:
    env = await WorkflowEnvironment.start_local(
        data_converter=pydantic_data_converter,
        dev_server_existing_path=shutil.which("temporal"),
    )
    task_queue = str(uuid.uuid4())
    handler = KnowledgeBaseServiceHandler()

    try:
        async with Worker(
            env.client,
            task_queue=task_queue,
            workflows=[KnowledgeBaseCallerWorkflow],
            nexus_service_handlers=[handler],
            workflow_runner=UnsandboxedWorkflowRunner(),
        ):
            endpoint = await env.create_nexus_endpoint(ENDPOINT, task_queue)
            try:
                result = await env.client.execute_workflow(
                    KnowledgeBaseCallerWorkflow.run,
                    id=str(uuid.uuid4()),
                    task_queue=task_queue,
                )
            finally:
                await env.delete_nexus_endpoint(endpoint)
    finally:
        await env.shutdown()

    assert result == {
        "blockId": "block-1",
        "categoryChildId": "child",
        "pageId": "page-1",
        "revision": 7,
    }
    assert [operation for operation, _ in handler.calls] == [
        "GetPage",
        "PutBlock",
        "GetCategoryTree",
    ]
