from __future__ import annotations

import json
from pathlib import Path
import typing
import uuid

from nexusrpc.handler import StartOperationContext, service_handler, sync_operation
from temporalio import workflow
from temporalio.api.common.v1 import Payload
from temporalio.contrib.pydantic import pydantic_data_converter
from temporalio.testing import WorkflowEnvironment
from temporalio.worker import UnsandboxedWorkflowRunner, Worker

from json_schema.api.kb import Block as ApiBlock
from json_schema.api.kb import Category as ApiCategory
from json_schema.api.kb import KnowledgeBaseService
from json_schema.api.kb import Page as ApiPage
from json_schema.api.kb.kb import models as api_kb_models
from json_schema.api.kb.kb import services as api_kb_services
from json_schema.definitions.kb import Block
from json_schema.definitions.kb import Category
from json_schema.definitions.kb import Page
from json_schema.definitions.kb.kb import models as kb_models

WIRE_FIXTURE_DIR = Path(__file__).resolve().parents[2] / "wire" / "json_schema" / "kb"


def load_fixture(name: str) -> object:
    return json.loads((WIRE_FIXTURE_DIR / name).read_text(encoding="utf-8"))


def fixture_bytes(name: str) -> bytes:
    return (WIRE_FIXTURE_DIR / name).read_bytes()


def roundtrip_fixture(name: str, model_type: type[typing.Any]) -> typing.Any:
    payload = Payload(
        metadata={"encoding": b"json/plain"},
        data=fixture_bytes(name),
    )
    converter = pydantic_data_converter.payload_converter
    model = converter.from_payloads([payload], [model_type])[0]
    encoded = converter.to_payloads([model])
    assert encoded is not None
    assert json.loads(encoded[0].data) == load_fixture(name)
    return model


@service_handler(service=api_kb_services.KnowledgeBaseService)
class KnowledgeBaseServiceHandler:
    def __init__(self) -> None:
        self.calls: list[tuple[str, object]] = []

    @sync_operation
    async def get_page(
        self,
        _ctx: StartOperationContext,
        input: api_kb_models.GetPageInput,
    ) -> ApiPage:
        self.calls.append(("GetPage", input))
        assert input.page_id == "page-1"
        return ApiPage.model_validate(load_fixture("page.json"))

    @sync_operation
    async def put_block(
        self,
        _ctx: StartOperationContext,
        input: ApiBlock,
    ) -> api_kb_models.PutBlockOutput:
        self.calls.append(("PutBlock", input))
        assert input.block_id == "block-1"
        assert input.style is not None
        assert input.style.bold is True
        return api_kb_models.PutBlockOutput.model_validate(
            load_fixture("put-block-output.json")
        )

    @sync_operation
    async def get_category_tree(
        self,
        _ctx: StartOperationContext,
        input: api_kb_models.GetCategoryTreeInput,
    ) -> ApiCategory:
        self.calls.append(("GetCategoryTree", input))
        assert input.root_id == "root"
        return ApiCategory.model_validate(load_fixture("category-tree.json"))


@workflow.defn
class KnowledgeBaseCallerWorkflow:
    @workflow.run
    async def run(self) -> dict[str, object]:
        service = KnowledgeBaseService("knowledge-base")

        page_handle = await service.get_page(
            api_kb_models.GetPageInput(pageId="page-1")
        )
        page = await page_handle

        block = page.blocks[0] if page.blocks is not None else None
        if block is None:
            raise RuntimeError("expected page block")
        put_block_handle = await service.put_block(block)
        put_block_output = await put_block_handle

        category_handle = await service.get_category_tree(
            api_kb_models.GetCategoryTreeInput(rootId="root")
        )
        category = await category_handle

        return {
            "blockId": put_block_output.block_id,
            "categoryChildId": category.children[0].id
            if category.children is not None
            else None,
            "pageId": page.page_id,
            "revision": put_block_output.revision,
        }


def test_kb_wire_fixtures_roundtrip_through_pydantic_converter() -> None:
    page = typing.cast(Page, roundtrip_fixture("page.json", Page))
    assert page.page_id == "page-1"
    assert page.blocks is not None
    assert page.blocks[0].block_id == "block-1"
    assert page.blocks[0].page is None
    assert page.blocks[0].style is not None
    assert page.blocks[0].style.bold is True

    block = typing.cast(
        Block,
        roundtrip_fixture("block.json", Block),
    )
    assert block.block_id == "block-1"
    assert block.page is None

    category = typing.cast(
        Category,
        roundtrip_fixture("category-tree.json", Category),
    )
    assert category.children is not None
    assert category.children[0].id == "child"

    request = typing.cast(
        kb_models.GetPageInput,
        roundtrip_fixture("get-page-input.json", kb_models.GetPageInput),
    )
    assert request.page_id == "page-1"

    category_request = typing.cast(
        kb_models.GetCategoryTreeInput,
        roundtrip_fixture(
            "get-category-tree-input.json", kb_models.GetCategoryTreeInput
        ),
    )
    assert category_request.root_id == "root"

    response = typing.cast(
        kb_models.PutBlockOutput,
        roundtrip_fixture("put-block-output.json", kb_models.PutBlockOutput),
    )
    assert response.revision == 7


async def test_kb_operations_use_real_nexus_client() -> None:
    env = await WorkflowEnvironment.start_local(  # pyright: ignore[reportUnknownMemberType]
        data_converter=pydantic_data_converter
    )
    task_queue = str(uuid.uuid4())
    service_handler = KnowledgeBaseServiceHandler()

    try:
        async with Worker(
            env.client,
            task_queue=task_queue,
            workflows=[KnowledgeBaseCallerWorkflow],
            nexus_service_handlers=[service_handler],
            workflow_runner=UnsandboxedWorkflowRunner(),
        ):
            endpoint = await env.create_nexus_endpoint("knowledge-base", task_queue)
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
    assert [call[0] for call in service_handler.calls] == [
        "GetPage",
        "PutBlock",
        "GetCategoryTree",
    ]
