"""Shared helpers for the generated JSON-Schema Python packages.

Generated models are plain ``@dataclasses.dataclass(slots=True, kw_only=True)``
types whose entire wire contract lives in a private
``_<Model>TransferTypeConverter`` registered through
``temporalio.converter.transfer_type_convertible``.

The Temporal SDK wraps *every* payload converter — including
``DataConverter.default`` — in ``_TemporalTransferTypePayloadConverter``, which
looks that converter up on the value's class. So the generated models round-trip
through the **default** data converter with no user wiring and no contrib
package. That is the load-bearing claim of this design, which is why
:func:`decode` and :func:`encode` go through
``temporalio.converter.DataConverter.default.payload_converter`` rather than
through the converter object directly.

:func:`converter_for` reaches the registered converter off the class the way
``advanced/samples/python/tests/test_start_workflow.py`` does. Negative tests use
it so the generated ``ValidationError`` surfaces unwrapped (the payload converter
would otherwise wrap it).

Round-trip assertions run on **bytes** (:func:`encode_bytes` against
:func:`canonical_fixture_bytes`), never on a parsed value: a parsed comparison
cannot see the wire form of a number, and `1 == 1.0` in Python, so it silently
admits an ``integer`` member that re-emitted as ``1.0``. :func:`encode` remains
available for the rare assertion that really is about structure.
"""

from __future__ import annotations

import json
from pathlib import Path
import typing

import temporalio.converter
from temporalio.api.common.v1 import Payload

T = typing.TypeVar("T")

#: Canonical cross-language wire fixtures, shared with the Go/TS/Java suites.
#: Never modify them — they are the polyglot contract.
WIRE_FIXTURE_ROOT = Path(__file__).resolve().parents[2] / "wire" / "json_schema"

#: The **complete** list of fixture members Python cannot re-emit, keyed by
#: ``(suite, fixture)`` and valued by the paths dropped from the expected wire.
#:
#: Every entry is one instance of the same documented exception: the
#: **optional+nullable collapse** (P1 exception (a), see
#: `specs/json-schema/features/nullability.md`). A dataclass has no presence
#: channel, so absent and an explicit wire ``null`` are the same in-memory state
#: (``None``) and both re-serialize as *omitted* — matching Go and Java, where the
#: same fixtures are verified by field checks for exactly this reason. TypeScript
#: is the only target that still round-trips the explicit ``null``.
#:
#: Nothing else belongs here. In particular a schema ``default`` is **not** an
#: entry: it is advisory, the field stays ``T | None = None`` with the value on a
#: ``DEFAULT_<FIELD>`` constant, so an unset defaulted key is omitted on the way
#: out exactly as it was absent on the way in — which is *why* the fixtures that
#: omit one (``showcase-minimal.json``, ``message-minimal.json``) round-trip
#: byte-identically, and the reason that design was chosen.
#:
#: A path is dot-separated; a ``[]`` segment means "every element of this array".
COLLAPSED_NULL_MEMBERS: dict[tuple[str, str], tuple[str, ...]] = {
    ("chat", "message-full.json"): ("replyToId",),
    ("kb", "block.json"): ("page",),
    ("kb", "page.json"): ("blocks[].page",),
    ("showcase", "showcase-nulls.json"): ("middleName",),
    ("temporal", "temporal-nulls.json"): ("deletedAt", "archivedOn"),
}

#: Fixtures that are **deliberately non-canonical input** — they exist to be
#: normalized, so their re-emitted bytes differ from their own. Not an exception
#: to round-trip fidelity: the value survives, only its spelling is canonicalized
#: (lowercase ``t``/``z`` → ``T``/``Z``, ``+00:00`` → ``Z``, ``PT90M`` →
#: ``PT1H30M``), identically in all four targets. The exact expected bytes are
#: asserted by the suite's own test (``test_temporal_canonicalization``).
NON_CANONICAL_FIXTURES: frozenset[tuple[str, str]] = frozenset(
    {("temporal", "temporal-canonicalize.json")}
)


def fixture_dir(suite: str) -> Path:
    """Directory holding one suite's canonical wire fixtures."""
    return WIRE_FIXTURE_ROOT / suite


def fixture_bytes(suite: str, name: str) -> bytes:
    """Raw bytes of a canonical wire fixture, exactly as they arrive on the wire."""
    return (fixture_dir(suite) / name).read_bytes()


def load_fixture(suite: str, name: str) -> typing.Any:
    """A canonical wire fixture parsed as a plain JSON value."""
    return json.loads((fixture_dir(suite) / name).read_text(encoding="utf-8"))


def converter_for(
    cls: type[T],
) -> temporalio.converter.TransferTypeConverter[T, typing.Any]:
    """The ``TransferTypeConverter`` the generator registered on ``cls``.

    Used by negative tests: calling ``from_transfer_type`` / ``to_transfer_type``
    directly surfaces the generated ``ValidationError`` (and its structured
    ``violations``) instead of whatever the payload converter wraps it in.
    """
    return typing.cast(
        "temporalio.converter.TransferTypeConverter[T, typing.Any]",
        getattr(cls, "__temporal_transfer_type_converter"),
    )


def _payload_converter() -> temporalio.converter.PayloadConverter:
    return temporalio.converter.DataConverter.default.payload_converter


def decode(cls: type[T], data: bytes) -> T:
    """Deserialize json/plain wire bytes into ``cls`` via the *default* converter."""
    payload = Payload(metadata={"encoding": b"json/plain"}, data=data)
    return _payload_converter().from_payloads([payload], [cls])[0]


def encode_bytes(model: object) -> bytes:
    """The exact wire bytes the *default* converter writes for ``model``.

    This is the payload's own ``data``, not a re-serialization of a parsed value,
    so it is the byte form a Go/TypeScript/Java peer would actually receive.
    """
    encoded = _payload_converter().to_payloads([model])
    assert encoded, "payload converter produced no payloads"
    return encoded[0].data


def encode(model: object) -> typing.Any:
    """Serialize a model via the *default* converter, returned as a JSON value.

    The parsed view, for the occasional assertion that is genuinely about
    structure. Round-trip assertions use :func:`encode_bytes` instead: a parsed
    comparison cannot distinguish ``1`` from ``1.0``.
    """
    return json.loads(encode_bytes(model))


def canonical_json_bytes(value: typing.Any) -> bytes:
    """``value`` in the byte form the SDK's JSON payload converter writes.

    ``JSONPlainPayloadConverter.to_payload`` serializes with
    ``separators=(",", ":"), sort_keys=True``, so its output is compact and
    key-sorted while a fixture file is formatted for humans. Canonicalizing the
    expectation through this function normalizes exactly those two properties —
    insignificant whitespace and member order, neither of which is part of the
    wire contract — while preserving everything that *is*:

    * the **numeric form**: ``json.loads("1.0")`` yields a ``float`` that
      ``json.dumps`` writes back as ``1.0``, and ``json.loads("1")`` an ``int``
      that writes back as ``1``, so an ``integer`` member that kept its wire
      ``float`` is caught here and nowhere else;
    * string escaping and every code point of every string;
    * the presence or absence of every key, at every depth.
    """
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def _drop_path(value: typing.Any, path: str) -> None:
    """Delete ``path`` from a parsed fixture, in place.

    ``a.b`` walks into an object member; ``a[].b`` walks into every element of an
    array member. A missing key raises, so a stale entry in
    :data:`COLLAPSED_NULL_MEMBERS` fails loudly rather than silently expecting the
    unmodified fixture.
    """
    segment, _, rest = path.partition(".")
    if segment.endswith("[]"):
        assert rest, f"an array segment needs a member after it: {path!r}"
        members = typing.cast("dict[str, typing.Any]", value)[segment[:-2]]
        for element in typing.cast("list[typing.Any]", members):
            _drop_path(element, rest)
        return
    mapping = typing.cast("dict[str, typing.Any]", value)
    if rest:
        _drop_path(mapping[segment], rest)
        return
    del mapping[segment]


def canonical_fixture_bytes(suite: str, name: str) -> bytes:
    """The bytes re-serializing a canonical wire fixture's model must produce.

    The fixture, canonicalized by :func:`canonical_json_bytes`, minus the members
    :data:`COLLAPSED_NULL_MEMBERS` documents Python cannot re-emit. Everything
    else must match byte for byte.
    """
    assert (suite, name) not in NON_CANONICAL_FIXTURES, (
        f"{suite}/{name} is deliberately non-canonical input; assert its expected"
        " bytes explicitly"
    )
    value = load_fixture(suite, name)
    for path in COLLAPSED_NULL_MEMBERS.get((suite, name), ()):
        _drop_path(value, path)
    return canonical_json_bytes(value)


def decode_fixture(cls: type[T], suite: str, name: str) -> T:
    """Deserialize a canonical wire fixture into ``cls`` via the default converter."""
    return decode(cls, fixture_bytes(suite, name))


def roundtrip_fixture(cls: type[T], suite: str, name: str) -> T:
    """Decode a canonical wire fixture and assert the re-emitted **bytes** match.

    The byte-level statement of P1: a payload one language accepts round-trips
    through any other unchanged. The comparison is against
    :func:`canonical_fixture_bytes`, never against a parsed value.

    The failure message spells both sides out: pytest rewrites assertions only in
    test modules, so a bare ``assert`` here would report nothing but
    ``AssertionError`` — and a one-character difference in a long payload is
    otherwise unfindable.
    """
    model = decode_fixture(cls, suite, name)
    emitted = encode_bytes(model)
    expected = canonical_fixture_bytes(suite, name)
    assert emitted == expected, (
        f"{suite}/{name} did not round-trip byte-identically\n"
        f"  emitted:  {emitted.decode()}\n"
        f"  expected: {expected.decode()}"
    )
    return model


def violation_pairs(error: typing.Any) -> list[tuple[str, str]]:
    """A generated ``ValidationError``'s violations as ``(path, reason)`` pairs.

    Aggregation (P11) is asserted on this list: one bad payload yields every
    violation it contains, in declared-property order.
    """
    return [(violation.path, violation.reason) for violation in error.violations]


def violation_paths(error: typing.Any) -> list[str]:
    """Just the paths of a generated ``ValidationError``'s violations."""
    return [violation.path for violation in error.violations]
