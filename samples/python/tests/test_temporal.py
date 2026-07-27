import datetime
import json
from pathlib import Path
import typing

import pytest
from temporalio.api.common.v1 import Payload
from temporalio.contrib.pydantic import pydantic_data_converter

from temporal import Temporal


WIRE_FIXTURE_DIR = (
    Path(__file__).resolve().parents[2] / "wire" / "json_schema" / "temporal"
)


def load_fixture(name: str) -> object:
    return json.loads((WIRE_FIXTURE_DIR / name).read_text(encoding="utf-8"))


def fixture_bytes(name: str) -> bytes:
    return (WIRE_FIXTURE_DIR / name).read_bytes()


def decode(name: str) -> Temporal:
    payload = Payload(metadata={"encoding": b"json/plain"}, data=fixture_bytes(name))
    converter = pydantic_data_converter.payload_converter
    return converter.from_payloads([payload], [Temporal])[0]


def encode(model: Temporal) -> object:
    converter = pydantic_data_converter.payload_converter
    encoded = converter.to_payloads([model])
    assert encoded is not None
    return json.loads(encoded[0].data)


def test_temporal_roundtrip_full() -> None:
    # Materialized temporals become native datetime/date/time/timedelta and
    # re-serialize (generator-owned) byte-identically for microsecond precision.
    model = decode("temporal-full.json")
    assert encode(model) == load_fixture("temporal-full.json")
    assert model.created_at.utcoffset() == datetime.timedelta(hours=2)
    assert model.created_at.microsecond == 123456
    assert model.timeout == datetime.timedelta(minutes=90)
    assert model.deleted_at is not None


def test_temporal_roundtrip_minimal() -> None:
    assert encode(decode("temporal-minimal.json")) == load_fixture(
        "temporal-minimal.json"
    )


def test_temporal_canonicalization() -> None:
    # Non-canonical input normalizes on re-serialize (uppercase T/Z, +00:00 -> Z,
    # PT90M -> PT1H30M).
    model = decode("temporal-canonicalize.json")
    assert encode(model) == {
        "createdAt": "2021-06-15T12:30:45Z",
        "birthday": "2021-02-28",
        "alarm": "12:30:45Z",
        "timeout": "PT1H30M",
    }


def test_temporal_nulls() -> None:
    model = decode("temporal-nulls.json")
    assert model.deleted_at is None
    assert model.archived_on is None
    assert model.timeout == datetime.timedelta(0)


@pytest.mark.parametrize(
    "field,value",
    [
        ("createdAt", "2021-12-31T23:59:60Z"),  # leap second
        ("timeout", "P1Y"),  # calendar duration
        ("birthday", "2021-02-29"),  # invalid calendar date
        ("createdAt", "2021-06-15T12:30:45"),  # missing offset
    ],
)
def test_temporal_materialized_narrowing_rejects(field: str, value: str) -> None:
    base: dict[str, typing.Any] = {
        "createdAt": "2021-06-15T12:30:45Z",
        "birthday": "2000-01-01",
        "alarm": "09:00:00",
        "timeout": "PT0S",
    }
    base[field] = value
    with pytest.raises(Exception):
        _ = Temporal.model_validate(base)
