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
    Labels,
    Settings,
    Showcase,
    Square,
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
