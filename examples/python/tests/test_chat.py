import json
from pathlib import Path
import typing

import pytest
from pydantic import ValidationError
from temporalio.api.common.v1 import Payload
from temporalio.contrib.pydantic import pydantic_data_converter

from json_schema.definitions.chat import (
    Labels,
    Message,
    Room,
    SendMessageInput,
    SendMessageOutput,
)


WIRE_FIXTURE_DIR = Path(__file__).resolve().parents[2] / "wire" / "json_schema" / "chat"


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


def test_optional_non_nullable_fields_reject_explicit_null() -> None:
    _ = Room(roomId="room-1", displayName="General", topic=None)

    with pytest.raises(ValidationError):
        _ = Room(roomId="room-1", displayName="General", topic=None, members=None)

    with pytest.raises(ValidationError):
        _ = Room(roomId="room-1", displayName="General", topic=None, labels=None)


def test_model_dump_omits_unset_defaults() -> None:
    message = Message(body="hello")
    assert message.model_dump(by_alias=True) == {"kind": "text", "body": "hello"}

    message_with_priority = Message(body="hello", priority=0)
    assert message_with_priority.model_dump(by_alias=True) == {
        "kind": "text",
        "body": "hello",
        "priority": 0,
    }


def test_model_dump_preserves_set_fields_and_extra_values() -> None:
    room = Room.model_validate(
        {"roomId": "room-1", "displayName": "General", "topic": None, "color": "blue"}
    )

    assert room.model_dump(by_alias=True) == {
        "roomId": "room-1",
        "displayName": "General",
        "topic": None,
        "color": "blue",
    }


def test_labels_validates_and_serializes_as_typed_map() -> None:
    labels = Labels.model_validate({"channel": "general", "team": "support"})
    assert labels.model_dump() == {"channel": "general", "team": "support"}

    with pytest.raises(ValidationError):
        _ = Labels.model_validate({"channel": 42})

    too_many = {f"key-{index}": "value" for index in range(51)}
    with pytest.raises(ValidationError):
        _ = Labels.model_validate(too_many)


def test_integer_fields_follow_json_schema_number_semantics() -> None:
    message = Message.model_validate({"body": "hello", "priority": 1.0})
    assert message.priority == 1

    with pytest.raises(ValidationError):
        _ = Message.model_validate({"body": "hello", "priority": True})

    with pytest.raises(ValidationError):
        _ = Message.model_validate({"body": "hello", "priority": 1.5})

    with pytest.raises(ValidationError):
        _ = Message.model_validate({"body": "hello", "priority": 2**53})


def test_canonical_wire_fixtures_roundtrip_through_temporal_pydantic_converter() -> (
    None
):
    message = typing.cast(Message, roundtrip_fixture("message-minimal.json", Message))
    assert message.kind == "text"
    assert message.body == "hi"
    assert message.reply_to_id is None
    assert message.priority == 0

    full_message = typing.cast(Message, roundtrip_fixture("message-full.json", Message))
    assert full_message.reply_to_id is None
    assert full_message.priority == 7

    room = typing.cast(Room, roundtrip_fixture("room-open.json", Room))
    assert room.room_id == "r1"
    assert room.model_extra == {"x-extra": 42}

    labels = typing.cast(Labels, roundtrip_fixture("labels.json", Labels))
    assert labels.model_extra == {"env": "prod", "team": "core"}

    request = typing.cast(
        SendMessageInput,
        roundtrip_fixture("send-message-input.json", SendMessageInput),
    )
    assert request.message.body == "hi"

    response = typing.cast(
        SendMessageOutput,
        roundtrip_fixture("send-message-output.json", SendMessageOutput),
    )
    assert response.message_id == "m1"
