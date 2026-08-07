from __future__ import annotations

import typing

import pytest
from temporalio.api.update.v1 import Outcome as ProtoOutcome
from temporalio.api.workflowservice.v1 import (
    PauseActivityRequest as ProtoPauseActivityRequest,
)
from temporalio.converter import PayloadConverter
import temporalio.nexus.system

from wit.proto_oneof import Outcome, PauseActivityRequest
from wit.proto_oneof.models import (
    _OutcomeTransferTypeConverter,
    _PauseActivityRequestTransferTypeConverter,
)


def test_proto_oneof_success_round_trip(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        temporalio.nexus.system,
        "_current_user_payload_converter",
        lambda: PayloadConverter.default,
    )
    converter = _OutcomeTransferTypeConverter()

    wire = converter.to_transfer_type(Outcome(value=("success", ["hello", 7])))
    assert wire.WhichOneof("value") == "success"

    decoded = converter.from_transfer_type(wire, Outcome)
    assert decoded == Outcome(value=("success", ["hello", 7]))


def test_required_proto_oneof_failure_round_trip() -> None:
    converter = _OutcomeTransferTypeConverter()

    wire = converter.to_transfer_type(Outcome(value=("failure", RuntimeError("boom"))))
    assert wire.WhichOneof("value") == "failure"
    decoded = converter.from_transfer_type(wire, Outcome)
    assert decoded.value is not None
    assert decoded.value[0] == "failure"
    assert str(decoded.value[1]).endswith("boom")


def test_required_proto_oneof_rejects_unset_wire_and_runtime_none() -> None:
    converter = _OutcomeTransferTypeConverter()

    with pytest.raises(ValueError, match="missing required field Outcome.value"):
        _ = converter.from_transfer_type(ProtoOutcome(), Outcome)

    invalid_model = Outcome(value=typing.cast(typing.Any, None))
    with pytest.raises(ValueError, match="missing required field Outcome.value"):
        _ = converter.to_transfer_type(invalid_model)


def test_optional_proto_oneof_round_trips_unset_as_none() -> None:
    converter = _PauseActivityRequestTransferTypeConverter()
    model = PauseActivityRequest(
        namespace="namespace",
        identity="worker",
        reason="maintenance",
        request_id="request-id",
    )

    wire = converter.to_transfer_type(model)
    assert wire.WhichOneof("activity") is None

    proto = ProtoPauseActivityRequest(
        namespace="namespace",
        identity="worker",
        reason="maintenance",
        request_id="request-id",
    )
    assert converter.from_transfer_type(proto, PauseActivityRequest) == model


def test_proto_oneof_rejects_unknown_authored_tag() -> None:
    converter = _OutcomeTransferTypeConverter()
    invalid_value = typing.cast(typing.Any, ("unknown", object()))

    with pytest.raises(ValueError, match="unknown protobuf oneof tag Outcome.value"):
        _ = converter.to_transfer_type(Outcome(value=invalid_value))
