from __future__ import annotations

import json
from pathlib import Path
import typing

from temporalio.api.common.v1 import Payload
from temporalio.contrib.pydantic import pydantic_data_converter

from kb import Block
from kb import Category
from kb import GetCategoryTreeInput
from kb import GetPageInput
from kb import Page
from kb import PutBlockOutput

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
        GetPageInput,
        roundtrip_fixture("get-page-input.json", GetPageInput),
    )
    assert request.page_id == "page-1"

    category_request = typing.cast(
        GetCategoryTreeInput,
        roundtrip_fixture("get-category-tree-input.json", GetCategoryTreeInput),
    )
    assert category_request.root_id == "root"

    response = typing.cast(
        PutBlockOutput,
        roundtrip_fixture("put-block-output.json", PutBlockOutput),
    )
    assert response.revision == 7
