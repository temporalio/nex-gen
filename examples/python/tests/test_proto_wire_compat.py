from __future__ import annotations

import base64
import datetime
import json
from pathlib import Path
import typing

import temporalio.common
from temporalio.api.common.v1 import Payload

from conftest import temporal_wire_data_converter
from wit.type_roundtrip.models import ActivityOptions


WIRE_FIXTURE_DIR = Path(__file__).resolve().parents[2] / "wire" / "proto"


def load_payload(example_id: str, name: str) -> Payload:
    fixture = json.loads(
        (WIRE_FIXTURE_DIR / example_id / name).read_text(encoding="utf-8")
    )
    metadata = {
        key: base64.b64decode(value)
        for key, value in typing.cast(dict[str, str], fixture["metadata"]).items()
    }
    return Payload(
        metadata=metadata,
        data=base64.b64decode(typing.cast(str, fixture["data"])),
    )


def assert_activity_options_model(decoded: object) -> None:
    assert isinstance(decoded, ActivityOptions)
    assert decoded.task_queue == "demo-task-queue"
    assert decoded.retry_policy.maximum_attempts == 3
    assert decoded.schedule_to_close_timeout == datetime.timedelta(seconds=7)
    assert decoded.priority == temporalio.common.Priority(
        priority_key=4,
        fairness_key="tenant-a",
        fairness_weight=2.5,
    )


def test_activity_options_temporal_wire_fixtures_decode() -> None:
    converter = temporal_wire_data_converter.payload_converter

    for fixture_name in (
        "activity-options.python.payload.json",
        "activity-options.dotnet.payload.json",
    ):
        payload = load_payload("type_roundtrip", fixture_name)
        decoded = converter.from_payloads([payload], [ActivityOptions])[0]
        assert_activity_options_model(decoded)
