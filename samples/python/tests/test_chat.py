import typing

import pytest

from chat import (
    Labels,
    Message,
    Room,
    SendMessageInput,
    SendMessageOutput,
)
from chat._definitions import ValidationError

from tests.json_converter_helper import (
    canonical_json_bytes,
    converter_for,
    encode_bytes,
    load_fixture,
    roundtrip_fixture,
    violation_pairs,
)

SUITE = "chat"


def expect_roundtrip(name: str, model_type: type[typing.Any]) -> typing.Any:
    """Decode a fixture through the default converter, re-encode, compare **bytes**.

    The members Python cannot re-emit — an explicit `null` on an optional+nullable
    member, which collapses (see
    :func:`test_message_full_optional_nullable_null_collapses`) — are declared once
    in ``COLLAPSED_NULL_MEMBERS``. Everything else matches byte for byte.
    """
    return roundtrip_fixture(model_type, SUITE, name)


def test_optional_non_nullable_members_reject_explicit_null() -> None:
    converter = converter_for(Room)

    # `topic` is required+nullable, so an explicit null IS the value.
    room = converter.from_transfer_type(
        {"roomId": "room-1", "displayName": "General", "topic": None}, Room
    )
    assert room.topic is None
    assert room.members is None
    assert room.labels is None
    assert room.additional_properties == {}

    # `members` and `labels` are optional and NON-nullable, so an explicit null is
    # a violation — and both are reported from the one payload (P11 aggregation),
    # in declared-property order.
    with pytest.raises(ValidationError) as excinfo:
        _ = converter.from_transfer_type(
            {
                "roomId": "room-1",
                "displayName": "General",
                "topic": None,
                "members": None,
                "labels": None,
            },
            Room,
        )
    assert violation_pairs(excinfo.value) == [
        ("members", "explicit null not allowed"),
        ("labels", "explicit null not allowed"),
    ]


def test_required_members_and_unknown_fields_aggregate() -> None:
    # A closed object reports every structural problem at once.
    with pytest.raises(ValidationError) as excinfo:
        _ = converter_for(SendMessageInput).from_transfer_type(
            {"extra": True}, SendMessageInput
        )
    assert violation_pairs(excinfo.value) == [
        ("roomId", "required"),
        ("message", "required"),
        ("extra", "unknown field"),
    ]

    with pytest.raises(ValidationError) as excinfo:
        _ = converter_for(SendMessageOutput).from_transfer_type({}, SendMessageOutput)
    assert violation_pairs(excinfo.value) == [("messageId", "required")]


def test_serialize_omits_unset_defaulted_members() -> None:
    # The property materializes `default: 0` on read, while its private backing
    # field retains the unset state used by the converter.
    converter = converter_for(Message)
    unset = Message(body="hello")
    assert unset.priority == 0
    assert converter.to_transfer_type(unset) == {"kind": "text", "body": "hello"}
    # The byte-level form of the claim: the wire the default-bearing member produces
    # is exactly the wire that omits it. Byte-identity is the whole justification for
    # keeping `default` off the dataclass field, so it is asserted on bytes.
    assert encode_bytes(unset) == canonical_json_bytes(
        {"kind": "text", "body": "hello"}
    )

    assert converter.to_transfer_type(Message(body="hello", priority=7)) == {
        "kind": "text",
        "body": "hello",
        "priority": 7,
    }
    # A set integer member emits as an integer, never as `7.0`.
    assert encode_bytes(Message(body="hello", priority=7)) == canonical_json_bytes(
        {"kind": "text", "body": "hello", "priority": 7}
    )
    # Explicitly assigning the schema default still marks the property present.
    unset.priority = 0
    assert converter.to_transfer_type(unset)["priority"] == 0
    # Deleting the property restores the unset state without changing the read value.
    del unset.priority
    assert unset.priority == 0
    assert "priority" not in converter.to_transfer_type(unset)
    # A `const` member, unlike a `default`, DOES carry its value as the dataclass
    # default — it is the only admissible value, not a suggestion.
    assert unset.kind == "text"


def test_default_property_materializes_without_changing_the_wire() -> None:
    message = converter_for(Message).from_transfer_type(
        {"kind": "text", "body": "hi"}, Message
    )
    assert message.priority == 0
    assert "priority" not in converter_for(Message).to_transfer_type(message)


def test_serialize_preserves_extras_and_required_nullable_null() -> None:
    room = converter_for(Room).from_transfer_type(
        {"roomId": "room-1", "displayName": "General", "topic": None, "color": "blue"},
        Room,
    )
    # An open object's undeclared keys land in the explicit catch-all member.
    assert room.additional_properties == {"color": "blue"}
    assert converter_for(Room).to_transfer_type(room) == {
        "roomId": "room-1",
        "displayName": "General",
        # required+nullable: the explicit null survives the round-trip.
        "topic": None,
        "color": "blue",
    }


def test_labels_validates_and_serializes_as_typed_map() -> None:
    converter = converter_for(Labels)

    labels = converter.from_transfer_type(
        {"channel": "general", "team": "support"}, Labels
    )
    assert labels.additional_properties == {"channel": "general", "team": "support"}
    assert converter.to_transfer_type(labels) == {
        "channel": "general",
        "team": "support",
    }
    # A map-shaped model is constructed through its catch-all member.
    assert converter.to_transfer_type(Labels(additional_properties={"a": "b"})) == {
        "a": "b"
    }

    with pytest.raises(ValidationError) as excinfo:
        _ = converter.from_transfer_type({"channel": 42}, Labels)
    assert violation_pairs(excinfo.value) == [("channel", "expected string")]

    too_many = {f"key-{index}": "value" for index in range(51)}
    with pytest.raises(ValidationError) as excinfo:
        _ = converter.from_transfer_type(too_many, Labels)
    assert violation_pairs(excinfo.value) == [
        ("", "must have at most 50 properties, got 51")
    ]


def test_integer_members_follow_json_schema_number_semantics() -> None:
    converter = converter_for(Message)

    # An integral JSON number is an integer.
    assert (
        converter.from_transfer_type(
            {"kind": "text", "body": "hi", "priority": 1.0}, Message
        ).priority
        == 1
    )

    # A boolean, a fractional number, and a value past the +/-(2**53-1) cap are
    # all "expected integer" — the single reason TypeScript uses for all three.
    for bad in (True, 1.5, 2**53):
        with pytest.raises(ValidationError) as excinfo:
            _ = converter.from_transfer_type(
                {"kind": "text", "body": "hi", "priority": bad}, Message
            )
        assert violation_pairs(excinfo.value) == [("priority", "expected integer")]


def test_const_member_rejects_a_wrong_wire_value() -> None:
    with pytest.raises(ValidationError) as excinfo:
        _ = converter_for(Message).from_transfer_type(
            {"kind": "image", "body": "hi"}, Message
        )
    assert violation_pairs(excinfo.value) == [("kind", 'must equal "text"')]


def test_canonical_wire_fixtures_roundtrip_through_the_default_converter() -> None:
    message = typing.cast(Message, expect_roundtrip("message-minimal.json", Message))
    assert message.kind == "text"
    assert message.body == "hi"
    assert message.reply_to_id is None
    # The public property materializes the default while the private presence
    # state remains unset and omitted on the way back out.
    assert message.priority == 0

    full_message = typing.cast(
        Message,
        expect_roundtrip("message-full.json", Message),
    )
    assert full_message.reply_to_id is None
    assert full_message.priority == 7

    room = typing.cast(Room, expect_roundtrip("room-open.json", Room))
    assert room.room_id == "r1"
    assert room.topic is None
    assert room.members == ["a"]
    assert room.additional_properties == {"x-extra": 42}

    labels = typing.cast(Labels, expect_roundtrip("labels.json", Labels))
    assert labels.additional_properties == {"env": "prod", "team": "core"}

    request = typing.cast(
        SendMessageInput,
        expect_roundtrip("send-message-input.json", SendMessageInput),
    )
    assert request.message.body == "hi"

    response = typing.cast(
        SendMessageOutput,
        expect_roundtrip("send-message-output.json", SendMessageOutput),
    )
    assert response.message_id == "m1"


def test_message_full_optional_nullable_null_collapses() -> None:
    # message-full.json carries `replyToId: null` on an optional+nullable member.
    # Absent and explicit null are the same in-memory state (None), and both
    # re-serialize as omitted, so the explicit null does NOT survive. Python now
    # matches Go and Java here (samples/go/tests/json_schema_chat_test.go verifies
    # message-full.json by field checks for exactly this reason); TypeScript is
    # the only target that still preserves it.
    wire = typing.cast(
        "dict[str, typing.Any]", load_fixture(SUITE, "message-full.json")
    )
    assert wire["replyToId"] is None

    converter = converter_for(Message)
    from_null = converter.from_transfer_type(wire, Message)
    from_absent = converter.from_transfer_type(
        {key: value for key, value in wire.items() if key != "replyToId"}, Message
    )
    assert from_null == from_absent
    assert "replyToId" not in converter.to_transfer_type(from_null)
