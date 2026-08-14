from __future__ import annotations

import typing

import pytest

from kb import Block
from kb import Category
from kb import GetCategoryTreeInput
from kb import GetPageInput
from kb import Page
from kb import PutBlockOutput
from kb._definitions import ValidationError

from tests.json_converter_helper import (
    converter_for,
    decode_fixture,
    encode,
    load_fixture,
    violation_pairs,
)

SUITE = "kb"


def expect_roundtrip(name: str, model_type: type[typing.Any]) -> typing.Any:
    """Decode a fixture through the default converter, re-encode, compare."""
    model = decode_fixture(model_type, SUITE, name)
    assert encode(model) == load_fixture(SUITE, name)
    return model


def test_kb_wire_fixtures_roundtrip_through_the_default_converter() -> None:
    category = typing.cast(Category, expect_roundtrip("category-tree.json", Category))
    assert category.children is not None
    assert category.children[0].id == "child"

    request = typing.cast(
        GetPageInput, expect_roundtrip("get-page-input.json", GetPageInput)
    )
    assert request.page_id == "page-1"

    category_request = typing.cast(
        GetCategoryTreeInput,
        expect_roundtrip("get-category-tree-input.json", GetCategoryTreeInput),
    )
    assert category_request.root_id == "root"

    response = typing.cast(
        PutBlockOutput, expect_roundtrip("put-block-output.json", PutBlockOutput)
    )
    assert response.revision == 7


def test_block_back_reference_null_collapses_on_roundtrip() -> None:
    # `Block.page` is the optional+nullable back-reference that terminates the
    # Page <-> Block cycle. A dataclass has no presence channel, so absent and
    # explicit `null` are the same in-memory state (None) and both re-serialize as
    # OMITTED — the explicit `"page": null` in the fixtures does not survive.
    # Python now matches Go and Java, which verify page.json/block.json by field
    # checks for exactly this reason (samples/go/tests/json_schema_kb_test.go).
    block = decode_fixture(Block, SUITE, "block.json")
    assert block.block_id == "block-1"
    assert block.order == 0
    assert block.page is None
    assert block.style is not None
    assert block.style.bold is True

    block_wire = typing.cast("dict[str, typing.Any]", load_fixture(SUITE, "block.json"))
    assert block_wire["page"] is None
    assert encode(block) == {
        key: value for key, value in block_wire.items() if key != "page"
    }

    page = decode_fixture(Page, SUITE, "page.json")
    assert page.page_id == "page-1"
    assert page.meta.author == "nexgen"
    assert page.blocks is not None
    assert page.blocks[0].block_id == "block-1"
    assert page.blocks[0].page is None
    assert page.blocks[0].style is not None
    assert page.blocks[0].style.bold is True

    page_wire = typing.cast("dict[str, typing.Any]", load_fixture(SUITE, "page.json"))
    expected_page = {**page_wire}
    expected_page["blocks"] = [
        {
            key: value
            for key, value in typing.cast("dict[str, typing.Any]", nested).items()
            if key != "page"
        }
        for nested in typing.cast("list[typing.Any]", page_wire["blocks"])
    ]
    assert encode(page) == expected_page


def test_nested_violations_carry_the_parent_path() -> None:
    # A nested `$ref` re-paths its violations under the parent member, and the
    # element index rides along for an array of models (P11 aggregation across
    # two different sub-objects of one payload).
    with pytest.raises(ValidationError) as excinfo:
        _ = converter_for(Page).from_transfer_type(
            {
                "pageId": "page-1",
                "title": "Runtime coverage",
                "meta": {},
                "blocks": [{"blockId": "block-1", "order": 0}, {"order": 1}],
            },
            Page,
        )
    assert violation_pairs(excinfo.value) == [
        ("meta.author", "required"),
        ("blocks[1].blockId", "required"),
    ]


def test_numeric_bound_on_a_nested_member() -> None:
    with pytest.raises(ValidationError) as excinfo:
        _ = converter_for(Block).from_transfer_type(
            {"blockId": "block-1", "order": -1}, Block
        )
    assert violation_pairs(excinfo.value) == [("order", "must be >= 0, got -1")]
