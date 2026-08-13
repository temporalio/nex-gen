import json
from pathlib import Path
import typing

import pytest
from pydantic import ValidationError
from temporalio.api.common.v1 import Payload
from temporalio.contrib.pydantic import pydantic_data_converter

from showcase import (
    Address,
    Attributes,
    Circle,
    ContactPy,
    Extras,
    Labels,
    LinkNote,
    Settings,
    Showcase,
    ShowcaseDetailObject,
    ShowcaseLedgerValue,
    Square,
    TextNote,
    Widget,
)


WIRE_FIXTURE_DIR = (
    Path(__file__).resolve().parents[2] / "wire" / "json_schema" / "showcase"
)


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


def test_const_default_and_reject_null_semantics() -> None:
    # Const `kind`/`revision`/`enabled` are injected when omitted and rejected
    # when wrong; enum fields (status/tier/scale) are required (no injection).
    minimal = Showcase.model_validate(
        {
            "name": "Widget",
            "count": 3,
            "active": True,
            "category": "tools",
            "status": "active",
            "tier": 1,
            "scale": 1.5,
        }
    )
    assert minimal.kind == "showcase"
    assert minimal.revision == 1
    assert minimal.enabled is True
    assert minimal.status == "active"
    assert minimal.tier == 1
    assert minimal.scale == 1.5
    # Default is present as a value but omitted from the serialized wire form.
    assert minimal.retries == 3
    assert minimal.model_dump(by_alias=True) == {
        "kind": "showcase",
        "revision": 1,
        "enabled": True,
        "status": "active",
        "tier": 1,
        "scale": 1.5,
        "name": "Widget",
        "count": 3,
        "active": True,
        "category": "tools",
    }

    with pytest.raises(ValidationError):
        _ = Showcase.model_validate(
            {"kind": "nope", "name": "w", "count": 1, "active": True, "category": None}
        )

    # A wrong integer const value is rejected.
    with pytest.raises(ValidationError) as const_exc:
        _ = Showcase.model_validate(
            {
                "revision": 2,
                "name": "w",
                "count": 1,
                "active": True,
                "category": "tools",
                "status": "active",
                "tier": 1,
                "scale": 1.5,
            }
        )
    assert "revision must equal 1" in str(const_exc.value)

    # Out-of-set enum values are rejected with an informative reason (the float
    # enum `scale` is plain `float`, closed only by the membership validator).
    enum_base = {
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
        "status": "active",
        "tier": 1,
        "scale": 1.5,
    }
    with pytest.raises(ValidationError) as status_exc:
        _ = Showcase.model_validate({**enum_base, "status": "archived"})
    assert "must be one of" in str(status_exc.value)
    with pytest.raises(ValidationError) as tier_exc:
        _ = Showcase.model_validate({**enum_base, "tier": 9})
    assert "must be one of [1, 2, 3]" in str(tier_exc.value)
    with pytest.raises(ValidationError) as scale_exc:
        _ = Showcase.model_validate({**enum_base, "scale": 3.5})
    assert "must be one of [1.5, 2.5]" in str(scale_exc.value)
    # A valid alternative member is accepted.
    ok = Showcase.model_validate(
        {**enum_base, "status": "pending", "tier": 3, "scale": 2.5}
    )
    assert ok.status == "pending"
    assert ok.scale == 2.5

    # An optional, non-nullable field rejects an explicit null.
    with pytest.raises(ValidationError):
        _ = Showcase.model_validate(
            {
                "name": "w",
                "count": 1,
                "active": True,
                "category": None,
                "nickname": None,
            }
        )


def test_labels_typed_map_and_settings_closed_object() -> None:
    labels = Labels.model_validate({"env": "prod", "team": "core"})
    assert labels.model_dump() == {"env": "prod", "team": "core"}

    with pytest.raises(ValidationError):
        _ = Labels.model_validate({"env": 42})

    with pytest.raises(ValidationError):
        _ = Settings.model_validate({"theme": "dark", "unknown": 1})


def test_canonical_wire_fixtures_roundtrip_through_temporal_pydantic_converter() -> (
    None
):
    minimal = typing.cast(
        Showcase, roundtrip_fixture("showcase-minimal.json", Showcase)
    )
    assert minimal.kind == "showcase"
    assert minimal.count == 3
    assert minimal.active is True
    assert minimal.category == "tools"
    assert minimal.retries == 3
    # Scalar defaults of each kind: absent on the wire, surfaced on read as the
    # native Pydantic field default; omitted on re-serialize (not in fields_set).
    assert minimal.greeting == "hello"
    assert minimal.debug is False
    assert "greeting" not in minimal.model_fields_set
    assert "debug" not in minimal.model_fields_set
    dumped = minimal.model_dump(by_alias=True)
    assert "greeting" not in dumped
    assert "debug" not in dumped
    assert "retries" not in dumped

    full = typing.cast(Showcase, roundtrip_fixture("showcase-full.json", Showcase))
    assert full.retries == 5
    assert full.middle_name == "Q"
    assert full.tags == ["a", "b"]
    assert full.aliases == ["alpha", "beta"]
    assert full.roles == ["admin", "user"]
    assert full.address is not None
    assert full.address.street == "1 Main St"
    assert full.address.model_extra == {"region": "west"}
    assert full.labels is not None
    assert full.labels.model_extra == {"env": "prod", "team": "core"}
    assert full.settings is not None
    assert full.settings.font_size == 14

    # Explicit nulls on nullable fields survive the round-trip in Python.
    nulls = typing.cast(Showcase, roundtrip_fixture("showcase-nulls.json", Showcase))
    assert nulls.middle_name is None
    assert nulls.category is None
    assert nulls.active is False

    address = typing.cast(Address, roundtrip_fixture("address-open.json", Address))
    assert address.street == "1 Main St"
    assert address.model_extra == {"x-extra": 7}

    labels = typing.cast(Labels, roundtrip_fixture("labels.json", Labels))
    assert labels.model_extra == {"env": "prod", "team": "core"}

    settings = typing.cast(Settings, roundtrip_fixture("settings.json", Settings))
    assert settings.theme == "dark"
    assert settings.font_size == 14


def test_numeric_constraints_roundtrip_and_reject() -> None:
    metrics = typing.cast(
        Showcase, roundtrip_fixture("showcase-metrics.json", Showcase)
    )
    assert metrics.priority == 5
    assert metrics.level == 2
    assert metrics.ratio == 15.0
    assert metrics.step == 9

    base = {
        "kind": "showcase",
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
        "status": "active",
        "tier": 1,
        "scale": 1.5,
    }

    # Integer above `maximum` (Pydantic's native Le already names the bound).
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "priority": 99})
    assert "less than or equal to 10" in str(excinfo.value)

    # Integer below `exclusiveMinimum`.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "level": 0})
    assert "greater than 0" in str(excinfo.value)

    # Integer that is not a multiple (native Pydantic multiple_of for ints).
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "step": 7})
    assert "multiple of 3" in str(excinfo.value)

    # Number that is not a multiple (explicit fmod AfterValidator, informative).
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "ratio": 7})
    assert "must be a multiple of 5, got 7" in str(excinfo.value)


def test_string_length_constraints_roundtrip_and_reject() -> None:
    # The astral crux: "a😀b" is 3 code points but 6 UTF-8 bytes; Pydantic's
    # native max_length counts code points, so it passes code (maxLength:5).
    strings = typing.cast(
        Showcase, roundtrip_fixture("showcase-strings.json", Showcase)
    )
    assert strings.code == "a😀b"
    assert strings.nickname == "buddy"

    base = {
        "kind": "showcase",
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
        "status": "active",
        "tier": 1,
        "scale": 1.5,
    }

    # A too-short `code` (1 code point, below minLength:2).
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "code": "a"})
    assert "at least 2 characters" in str(excinfo.value)

    # An over-long `code` (6 code points, above maxLength:5).
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "code": "abcdef"})
    assert "at most 5 characters" in str(excinfo.value)

    # Astral: 6 emoji = 6 code points (24 bytes); rejected by code-point count.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "code": "😀😀😀😀😀😀"})
    assert "at most 5 characters" in str(excinfo.value)

    # A multi-byte value within the code-point bound is accepted (byte count 6
    # would exceed maxLength:5 — proving code points, not bytes).
    ok = Showcase.model_validate({**base, "code": "a😀b"})
    assert ok.code == "a😀b"


def test_pattern_constraints_roundtrip_and_reject() -> None:
    # sku `^[A-Z]{2,4}$` and phrase `^\S+\s\S+$` round-trip.
    patterns = typing.cast(
        Showcase, roundtrip_fixture("showcase-patterns.json", Showcase)
    )
    assert patterns.sku == "AB"
    assert patterns.phrase == "hello world"

    base = {
        "kind": "showcase",
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
        "status": "active",
        "tier": 1,
        "scale": 1.5,
    }

    # Lowercase sku (not [A-Z]).
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "sku": "ab"})
    assert "must match pattern" in str(excinfo.value)

    # Too-long sku (5 letters).
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "sku": "ABCDE"})
    assert "must match pattern" in str(excinfo.value)

    # phrase with no whitespace separator.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "phrase": "helloworld"})
    assert "must match pattern" in str(excinfo.value)

    # `\s` ASCII-class crux: a NBSP (U+00A0) is not ASCII whitespace, so the
    # normalized `[\t\n\x0B\f\r ]` (matched with re.ASCII) rejects it — matching
    # Go/TS/Java (JS's native Unicode `\s` would otherwise have accepted it).
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "phrase": "hello world"})
    assert "must match pattern" in str(excinfo.value)

    # `$` end-anchor crux: a trailing newline is rejected. Python `re`'s `$`
    # matches before a trailing `\n`; the loader rewrote `$`→`\Z` (strict end)
    # so this rejects, consistent with Go/TS/Java.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "phrase": "hello world\n"})
    assert "must match pattern" in str(excinfo.value)

    # A valid ASCII-space phrase and sku are accepted.
    ok = Showcase.model_validate({**base, "sku": "XY", "phrase": "hello world"})
    assert ok.sku == "XY"
    assert ok.phrase == "hello world"


def test_format_constraints_roundtrip_and_reject() -> None:
    # uuid/email/hostname/uri/ipv4 round-trip (string-typed, no materialization).
    formats = typing.cast(Showcase, roundtrip_fixture("showcase-format.json", Showcase))
    assert formats.request_id == "de305d54-75b4-431b-adb2-eb6b9e546013"
    assert formats.contact_email == "user@example.com"
    assert formats.host == "api.example.com"
    assert formats.homepage == "https://example.com/path?q=1#frag"
    assert formats.gateway == "192.168.0.1"

    base = {
        "kind": "showcase",
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
        "status": "active",
        "tier": 1,
        "scale": 1.5,
    }

    # A malformed uuid.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "requestId": "not-a-uuid"})
    assert "must be a valid uuid" in str(excinfo.value)

    # Single-label email domain (user@localhost) is rejected.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "contactEmail": "user@localhost"})
    assert "must be a valid email" in str(excinfo.value)

    # ipv4 octet out of range.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "gateway": "256.0.0.1"})
    assert "must be a valid ipv4" in str(excinfo.value)

    # uri with a double-`::` IPv6 IP-literal host (spliced ipv6 grammar rejects).
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "homepage": "http://[1::2::3]"})
    assert "must be a valid uri" in str(excinfo.value)

    # An over-long hostname (> 253 code points) is rejected by the length guard.
    long_host = ".".join(["abc"] * 64)
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "host": long_host})
    assert "must be a valid hostname" in str(excinfo.value)


def test_array_constraints_roundtrip_and_reject() -> None:
    base = {
        "kind": "showcase",
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
        "status": "active",
        "tier": 1,
        "scale": 1.5,
    }

    # Valid arrays are accepted.
    ok = Showcase.model_validate(
        {**base, "tags": ["a"], "aliases": ["x", "y"], "roles": ["admin"]}
    )
    assert ok.roles == ["admin"]

    # Too few items (minItems:1) — Pydantic's native min_length names the bound.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "tags": []})
    assert "at least 1 item" in str(excinfo.value)

    # Too many items (maxItems:5).
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "tags": ["a", "b", "c", "d", "e", "f"]})
    assert "at most 5 item" in str(excinfo.value)

    # Duplicate element (uniqueItems).
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "aliases": ["x", "x"]})
    assert "duplicate items: element at index 1 equals index 0" in str(excinfo.value)

    # Missing required contains match (no "admin").
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "roles": ["user"]})
    assert "too few matching items: at least 1, got 0" in str(excinfo.value)

    # Too many contains matches (maxContains:2).
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "roles": ["admin", "admin", "admin"]})
    assert "too many matching items: at most 2, got 3" in str(excinfo.value)


def test_object_constraints_roundtrip_and_reject() -> None:
    # Valid map and object round-trip through the Temporal converter.
    attributes = typing.cast(
        Attributes, roundtrip_fixture("attributes.json", Attributes)
    )
    assert attributes.model_extra == {"host": "a", "port": "8080"}
    contact = typing.cast(ContactPy, roundtrip_fixture("contact.json", ContactPy))
    assert contact.shipping_street == "1 Main St"
    assert contact.shipping_zip == "90210"

    # minProperties:1 on a map — an empty object is too few (counted over the
    # distinct wire keys via model_fields_set, never a declared+extras sum).
    with pytest.raises(ValidationError) as excinfo:
        _ = Attributes.model_validate({})
    assert "must have at least 1 properties, got 0" in str(excinfo.value)

    # maxProperties:3 on a map.
    with pytest.raises(ValidationError) as excinfo:
        _ = Attributes.model_validate({"a": "1", "b": "2", "c": "3", "d": "4"})
    assert "must have at most 3 properties, got 4" in str(excinfo.value)

    # propertyNames maxLength:8 — an over-long key (code-point length).
    with pytest.raises(ValidationError) as excinfo:
        _ = Attributes.model_validate({"toolongkey": "1"})
    assert 'invalid property name "toolongkey": must have length <= 8, got 10' in str(
        excinfo.value
    )

    # dependentRequired — a shipping street present without a shipping zip.
    with pytest.raises(ValidationError) as excinfo:
        _ = ContactPy.model_validate({"shippingStreet": "1 Main St"})
    assert 'property "shippingZip" is required when "shippingStreet" is present' in str(
        excinfo.value
    )

    # minProperties:1 on a declared-property object — an empty object.
    with pytest.raises(ValidationError) as excinfo:
        _ = ContactPy.model_validate({})
    assert "must have at least 1 properties, got 0" in str(excinfo.value)

    # A satisfied dependency validates.
    ok = ContactPy.model_validate(
        {"shippingStreet": "1 Main St", "shippingZip": "90210"}
    )
    assert ok.shipping_zip == "90210"


def test_all_of_merged_widget() -> None:
    # Widget is an allOf base-type extension (WidgetBase folded in + an extension
    # branch): a flat standalone object with the union of properties ({id, kind,
    # name, size}) and required ([id, name]), with no allOf residue.
    widget = roundtrip_fixture("widget.json", Widget)
    assert widget.id == "w-1"
    assert widget.kind == "gadget"
    assert widget.name == "Widget One"
    assert widget.size == 15

    # `size` carries a bound tightened from two allOf branches to [10, 20].
    with pytest.raises(ValidationError):
        _ = Widget.model_validate({"id": "w-1", "name": "Widget One", "size": 5})
    with pytest.raises(ValidationError):
        _ = Widget.model_validate({"id": "w-1", "name": "Widget One", "size": 25})

    # A missing required member contributed by the extension branch is rejected.
    with pytest.raises(ValidationError):
        _ = Widget.model_validate({"id": "w-1"})

    # A value on the tightened boundary validates.
    ok = Widget.model_validate({"id": "w-1", "name": "Widget One", "size": 10})
    assert ok.size == 10


def test_one_of_sum_types_roundtrip_and_reject() -> None:
    # Disjoint-kind union (str | int): each branch round-trips and is selected
    # by the wire token (Pydantic smart-union mode).
    as_string = typing.cast(
        Showcase, roundtrip_fixture("showcase-union-string.json", Showcase)
    )
    assert as_string.id_or_name == "abc"
    as_int = typing.cast(
        Showcase, roundtrip_fixture("showcase-union-int.json", Showcase)
    )
    assert as_int.id_or_name == 7

    # Discriminated (tagged) union (Circle | Square) selected by `kind`.
    circle = typing.cast(
        Showcase, roundtrip_fixture("showcase-shape-circle.json", Showcase)
    )
    assert isinstance(circle.shape, Circle)
    assert circle.shape.kind == "circle"
    assert circle.shape.radius == 2.5
    square = typing.cast(
        Showcase, roundtrip_fixture("showcase-shape-square.json", Showcase)
    )
    assert isinstance(square.shape, Square)
    assert square.shape.kind == "square"
    assert square.shape.side == 4

    base = {
        "kind": "showcase",
        "revision": 1,
        "enabled": True,
        "status": "active",
        "tier": 1,
        "scale": 1.5,
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
    }

    # An unmatchable wire token (boolean) matches no branch of str | int.
    with pytest.raises(ValidationError):
        _ = Showcase.model_validate({**base, "idOrName": True})

    # An unknown discriminator value is rejected (closed value set, P13.1).
    with pytest.raises(ValidationError):
        _ = Showcase.model_validate({**base, "shape": {"kind": "triangle"}})


def test_free_form_object_roundtrip_and_reject() -> None:
    # The free-form object in both positions: the inline object branch of the
    # `payload` union, and the named `Extras` model. Members are carried
    # verbatim, so a large integer survives untruncated.
    as_object = typing.cast(
        Showcase, roundtrip_fixture("showcase-freeform.json", Showcase)
    )
    assert isinstance(as_object.payload, dict)
    assert as_object.payload["big"] == 9007199254740992
    assert as_object.extras is not None
    assert (as_object.extras.model_extra or {})["note"] == "free-form"

    # The same union's string branch, selected by the wire token.
    as_string = typing.cast(
        Showcase, roundtrip_fixture("showcase-freeform-string.json", Showcase)
    )
    assert as_string.payload == "text"

    # The named free-form model round-trips standalone, nested members included.
    extras = typing.cast(Extras, roundtrip_fixture("extras.json", Extras))
    members = extras.model_extra or {}
    assert members["nested"] == {"a": 1}
    assert members["count"] == 9007199254740992

    # maxProperties over the member set is enforced.
    with pytest.raises(ValidationError) as excinfo:
        _ = Extras.model_validate({"a": 1, "b": 2, "c": 3, "d": 4, "e": 5})
    assert "must have at most 4 properties" in str(excinfo.value)

    base = {
        "kind": "showcase",
        "revision": 1,
        "enabled": True,
        "status": "active",
        "tier": 1,
        "scale": 1.5,
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
    }

    # An unmatchable wire token (boolean) matches no branch of object | string.
    with pytest.raises(ValidationError):
        _ = Showcase.model_validate({**base, "payload": True})


def test_inline_object_union_roundtrip_and_reject() -> None:
    # The `note` tagged union's branches are written inline in the schema and
    # named by their `x-py-name` overrides, so each is a `BaseModel` Pydantic
    # selects on — with its own constraints and its own open member set.
    text = typing.cast(Showcase, roundtrip_fixture("showcase-note-text.json", Showcase))
    assert isinstance(text.note, TextNote)
    assert text.note.kind == "text"
    assert text.note.body == "remember the milk"
    # The branch stays open: an unknown member is preserved (P13).
    assert (text.note.model_extra or {})["pinned"] is True

    link = typing.cast(Showcase, roundtrip_fixture("showcase-note-link.json", Showcase))
    assert isinstance(link.note, LinkNote)
    assert link.note.href == "https://example.test/notes/1"

    base = {
        "kind": "showcase",
        "revision": 1,
        "enabled": True,
        "status": "active",
        "tier": 1,
        "scale": 1.5,
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
    }

    # The selected branch's own constraints are enforced.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "note": {"kind": "text", "body": ""}})
    assert "at least 1 character" in str(excinfo.value)

    # An unknown tag value matches no branch.
    with pytest.raises(ValidationError):
        _ = Showcase.model_validate({**base, "note": {"kind": "audio"}})


def test_property_inline_object_union_roundtrip_and_reject() -> None:
    # `detail`'s union is written inline on the property; its lone structured
    # object branch derives `ShowcaseDetailObject` from the union it belongs to
    # and is an ordinary model, so Pydantic selects on it by shape.
    object_detail = typing.cast(
        Showcase, roundtrip_fixture("showcase-detail-object.json", Showcase)
    )
    assert isinstance(object_detail.detail, ShowcaseDetailObject)
    assert object_detail.detail.code == "E_LIMIT"
    assert object_detail.detail.hint == "retry later"
    # The branch stays open: an unknown member is preserved (P13).
    assert (object_detail.detail.model_extra or {})["retryAfterMs"] == 250

    text = typing.cast(
        Showcase, roundtrip_fixture("showcase-detail-string.json", Showcase)
    )
    assert text.detail == "E_LIMIT"

    base = {
        "kind": "showcase",
        "revision": 1,
        "enabled": True,
        "status": "active",
        "tier": 1,
        "scale": 1.5,
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
    }

    # The object branch's own constraints are enforced.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "detail": {"code": ""}})
    assert "at least 1 character" in str(excinfo.value)

    # A value admitted by no branch is rejected.
    with pytest.raises(ValidationError):
        _ = Showcase.model_validate({**base, "detail": 7})


def test_tagged_union_with_scalar_branch_roundtrip_and_reject() -> None:
    # `shapeOrName` composes both selector layers: the JSON token picks
    # object-vs-string, then the `kind` const picks Circle-vs-Square. Both branch
    # models are the ones the `shape` union already uses.
    square = typing.cast(
        Showcase, roundtrip_fixture("showcase-shape-or-name-square.json", Showcase)
    )
    assert isinstance(square.shape_or_name, Square)
    assert square.shape_or_name.side == 4

    named = typing.cast(
        Showcase, roundtrip_fixture("showcase-shape-or-name-string.json", Showcase)
    )
    assert named.shape_or_name == "unit-square"

    base = {
        "kind": "showcase",
        "revision": 1,
        "enabled": True,
        "status": "active",
        "tier": 1,
        "scale": 1.5,
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
    }

    # An object with an unknown tag matches no branch — it does not fall back to
    # the string branch.
    with pytest.raises(ValidationError):
        _ = Showcase.model_validate({**base, "shapeOrName": {"kind": "triangle"}})

    # A value admitted by no branch is rejected.
    with pytest.raises(ValidationError):
        _ = Showcase.model_validate({**base, "shapeOrName": 7})


def test_array_branch_union_roundtrip_and_reject() -> None:
    # `measurements` is `list[float] | str`: Python carries the array branch
    # structurally (no synthesized variant model) and Pydantic selects by kind.
    values = typing.cast(
        Showcase, roundtrip_fixture("showcase-measurements-array.json", Showcase)
    )
    assert values.measurements == [1.5, 2.5, 3.75]

    preset = typing.cast(
        Showcase, roundtrip_fixture("showcase-measurements-string.json", Showcase)
    )
    assert preset.measurements == "auto"

    base = {
        "kind": "showcase",
        "revision": 1,
        "enabled": True,
        "status": "active",
        "tier": 1,
        "scale": 1.5,
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
    }

    # A value admitted by neither branch is rejected.
    with pytest.raises(ValidationError):
        _ = Showcase.model_validate({**base, "measurements": True})


def test_element_position_unions_roundtrip_and_reject() -> None:
    # Unions in positions with no property of their own: an array element at a
    # named union (`shapes`), an array element at an inline union the loader
    # names `ShowcaseSegmentsItem`, and a map member at an inline union named
    # `ChoicesValue`. Pydantic selects the branch per element/member.
    value = typing.cast(
        Showcase, roundtrip_fixture("showcase-element-unions.json", Showcase)
    )
    assert value.shapes is not None
    assert isinstance(value.shapes[0], Circle)
    assert value.shapes[0].radius == 2.5
    assert isinstance(value.shapes[1], Square)
    assert value.shapes[1].side == 4
    assert value.segments == ["alpha", 7]
    # Element nullability is the element's own concern: `list[str | None]`, so
    # an explicit null is a member rather than a violation.
    assert value.slots == ["first", None, "third"]
    # A map's members live in Pydantic's extras bag, materialized into their
    # declared member type — here the union each member is routed to.
    assert value.choices is not None
    assert value.choices.model_extra is not None
    primary = value.choices.model_extra["primary"]
    assert isinstance(primary, Circle)
    assert primary.radius == 1

    base = {
        "kind": "showcase",
        "revision": 1,
        "enabled": True,
        "status": "active",
        "tier": 1,
        "scale": 1.5,
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
    }

    # An element admitted by no branch is rejected, at its own index.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate(
            {**base, "shapes": [{"kind": "circle", "radius": 1}, True]}
        )
    assert "shapes" in str(excinfo.value)

    with pytest.raises(ValidationError):
        _ = Showcase.model_validate({**base, "segments": ["ok", 1.5]})


def test_content_encoding_roundtrip_and_reject() -> None:
    # blob (base64) and urlBlob (base64url) round-trip: JSON string on the wire,
    # native `bytes` in the model, re-encoded byte-identically. The same bytes
    # (">>>") encode differently per encoding ("Pj4+" vs "Pj4-").
    parsed = typing.cast(Showcase, roundtrip_fixture("showcase-bytes.json", Showcase))
    assert parsed.blob == b">>>"
    assert parsed.url_blob == b">>>"

    base = {
        "kind": "showcase",
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
        "status": "active",
        "tier": 1,
        "scale": 1.5,
    }

    # A base64 field using the URL-safe alphabet is rejected by the pinned regex.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "blob": "Pj4-"})
    assert "must be base64-encoded" in str(excinfo.value)

    # A base64 field missing padding is rejected.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "blob": "aGk"})
    assert "must be base64-encoded" in str(excinfo.value)

    # A base64url field carrying padding is rejected.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "urlBlob": "aGk="})
    assert "must be base64url-encoded" in str(excinfo.value)


def test_inline_object_shapes_roundtrip_and_reject() -> None:
    # An object written inline in a value position is named after that position
    # and emitted as an ordinary model: a property (`location`, with its own
    # nested `geo`), a nullable property (`audit`), an array element (`rows`), a
    # map and its member (`ledger`), and a free-form bag (`metadata`). The same
    # fixture covers a typed map's member constraints (`quotas`, `tokens`,
    # `nicknames`) and a nested array (`grid`).
    value = typing.cast(
        Showcase, roundtrip_fixture("showcase-inline-shapes.json", Showcase)
    )
    assert value.grid == [[1, 2], [3]]
    assert value.location is not None
    assert value.location.city == "Springfield"
    assert value.location.geo is not None
    assert value.location.geo.lat == 39.8
    assert value.audit is not None
    assert value.audit.by == "alice"
    assert value.rows is not None
    assert value.rows[0].cell == "a1"
    # The member override renamed the member (`ledger_py`); the hoisted types keep
    # their position-derived names. A map's members are materialized into their
    # declared member type.
    assert value.ledger_py is not None
    opening = (value.ledger_py.model_extra or {})["opening"]
    assert isinstance(opening, ShowcaseLedgerValue)
    assert opening.amount == 100
    assert value.metadata is not None
    assert (value.metadata.model_extra or {}) == {"source": "import", "batch": 7}
    assert value.quotas is not None
    assert (value.quotas.model_extra or {}) == {"cpu": 20, "memory": 100}
    # A null member of a nullable map is a member, not a violation.
    assert value.nicknames is not None
    assert (value.nicknames.model_extra or {}) == {"short": "al", "none": None}

    base = {
        "kind": "showcase",
        "name": "w",
        "count": 1,
        "active": True,
        "category": "tools",
        "status": "active",
        "tier": 1,
        "scale": 1.5,
    }

    # A hoisted shape validates like any other model, at the nested path.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "location": {"city": ""}})
    assert "location.city" in str(excinfo.value)

    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "rows": [{"cell": "ok"}, {}]})
    assert "rows.1.cell" in str(excinfo.value)

    # A nested array reports the failing element at its own two-dimensional index.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "grid": [[1], [2, 1.5]]})
    assert "grid.1.1" in str(excinfo.value)

    # A typed map's member constraints are enforced, keyed by the member.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "quotas": {"cpu": 7}})
    assert "cpu" in str(excinfo.value)

    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "tokens": {"primary": "AB"}})
    assert "primary" in str(excinfo.value)

    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate({**base, "nicknames": {"tiny": "a"}})
    assert "tiny" in str(excinfo.value)

    # The free-form bag's member-count bound rides with the hoisted type.
    with pytest.raises(ValidationError) as excinfo:
        _ = Showcase.model_validate(
            {**base, "metadata": {"a": 1, "b": 2, "c": 3, "d": 4}}
        )
    assert "at most 3 properties" in str(excinfo.value)
