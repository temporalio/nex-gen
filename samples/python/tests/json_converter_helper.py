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


def encode(model: object) -> typing.Any:
    """Serialize a model via the *default* converter, returned as a JSON value."""
    encoded = _payload_converter().to_payloads([model])
    assert encoded, "payload converter produced no payloads"
    return json.loads(encoded[0].data)


def decode_fixture(cls: type[T], suite: str, name: str) -> T:
    """Deserialize a canonical wire fixture into ``cls`` via the default converter."""
    return decode(cls, fixture_bytes(suite, name))


def violation_pairs(error: typing.Any) -> list[tuple[str, str]]:
    """A generated ``ValidationError``'s violations as ``(path, reason)`` pairs.

    Aggregation (P11) is asserted on this list: one bad payload yields every
    violation it contains, in declared-property order.
    """
    return [(violation.path, violation.reason) for violation in error.violations]


def violation_paths(error: typing.Any) -> list[str]:
    """Just the paths of a generated ``ValidationError``'s violations."""
    return [violation.path for violation in error.violations]
