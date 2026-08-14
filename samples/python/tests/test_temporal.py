import datetime
import typing

import pytest

from temporal import Temporal
from temporal._definitions import ValidationError

from tests.json_converter_helper import (
    converter_for,
    decode_fixture,
    encode,
    load_fixture,
    violation_pairs,
)

SUITE = "temporal"


def decode(name: str) -> Temporal:
    return decode_fixture(Temporal, SUITE, name)


def test_temporal_roundtrip_full() -> None:
    # Materialized temporals become native datetime/date/time/timedelta and
    # re-serialize (generator-owned) byte-identically for microsecond precision.
    model = decode("temporal-full.json")
    assert encode(model) == load_fixture(SUITE, "temporal-full.json")
    assert model.created_at.utcoffset() == datetime.timedelta(hours=2)
    assert model.created_at.microsecond == 123456
    assert model.timeout == datetime.timedelta(minutes=90)
    assert model.deleted_at is not None


def test_temporal_roundtrip_minimal() -> None:
    assert encode(decode("temporal-minimal.json")) == load_fixture(
        SUITE, "temporal-minimal.json"
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


def test_temporal_nulls_collapse_on_roundtrip() -> None:
    # `deletedAt`/`archivedOn` are optional+nullable. A dataclass has no presence
    # channel, so absent and explicit `null` are the same in-memory state (None)
    # and both re-serialize as OMITTED. Python now matches Go and Java here (see
    # samples/go/tests/json_schema_temporal_test.go, TestJSONSchemaTemporalNulls);
    # only TypeScript still preserves the explicit null.
    model = decode("temporal-nulls.json")
    assert model.deleted_at is None
    assert model.archived_on is None
    assert model.timeout == datetime.timedelta(0)

    wire = typing.cast(
        "dict[str, typing.Any]", load_fixture(SUITE, "temporal-nulls.json")
    )
    assert wire["deletedAt"] is None
    assert wire["archivedOn"] is None
    # The explicit nulls are gone from the re-encoded wire; everything else survives.
    assert encode(model) == {
        key: value
        for key, value in wire.items()
        if key not in ("deletedAt", "archivedOn")
    }


def test_temporal_absent_and_explicit_null_are_indistinguishable() -> None:
    # The collapse, stated directly: the two payloads produce equal models.
    base: dict[str, typing.Any] = {
        "createdAt": "2021-06-15T12:30:45Z",
        "birthday": "2000-01-01",
        "alarm": "09:00:00",
        "timeout": "PT0S",
    }
    converter = converter_for(Temporal)
    absent = converter.from_transfer_type(base, Temporal)
    explicit_null = converter.from_transfer_type(
        {**base, "deletedAt": None, "archivedOn": None}, Temporal
    )
    assert absent == explicit_null


def test_missing_required_members_aggregate() -> None:
    # P11: one bad payload surfaces every violation it contains, in declared
    # property order — the aggregation pydantic used to provide for free.
    with pytest.raises(ValidationError) as excinfo:
        _ = converter_for(Temporal).from_transfer_type({}, Temporal)
    assert violation_pairs(excinfo.value) == [
        ("createdAt", "required"),
        ("birthday", "required"),
        ("alarm", "required"),
        ("timeout", "required"),
    ]


def test_non_object_payload_is_a_single_structural_violation() -> None:
    with pytest.raises(ValidationError) as excinfo:
        _ = converter_for(Temporal).from_transfer_type("nope", Temporal)
    assert violation_pairs(excinfo.value) == [("", "expected object")]


def test_unknown_member_is_rejected() -> None:
    with pytest.raises(ValidationError) as excinfo:
        _ = converter_for(Temporal).from_transfer_type(
            {
                "createdAt": "2021-06-15T12:30:45Z",
                "birthday": "2000-01-01",
                "alarm": "09:00:00",
                "timeout": "PT0S",
                "nope": 1,
            },
            Temporal,
        )
    assert violation_pairs(excinfo.value) == [("nope", "unknown field")]


@pytest.mark.parametrize(
    "field,value,format_name",
    [
        ("createdAt", "2021-12-31T23:59:60Z", "date-time"),  # leap second
        ("timeout", "P1Y", "duration"),  # calendar duration
        ("birthday", "2021-02-29", "date"),  # invalid calendar date
        ("createdAt", "2021-06-15T12:30:45", "date-time"),  # missing offset
    ],
)
def test_temporal_materialized_narrowing_rejects(
    field: str, value: str, format_name: str
) -> None:
    base: dict[str, typing.Any] = {
        "createdAt": "2021-06-15T12:30:45Z",
        "birthday": "2000-01-01",
        "alarm": "09:00:00",
        "timeout": "PT0S",
    }
    base[field] = value
    with pytest.raises(ValidationError) as excinfo:
        _ = converter_for(Temporal).from_transfer_type(base, Temporal)
    # The reason names the format and the offending value, rendered in its JSON
    # form exactly as Go and TypeScript render it.
    assert violation_pairs(excinfo.value) == [
        (field, f'must be a valid {format_name}, got "{value}"')
    ]
